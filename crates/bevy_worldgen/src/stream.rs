//! Chunk state machine, planning, dispatch, collection and eviction.

use crate::origin::WorldOrigin;
use bevy::math::DVec3;
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use worldgen_core::{CancelToken, ChunkId, GenVersion, WorldBudget};
use worldgen_jobs::JobPool;
use worldgen_world::GeneratedChunk;
use worldgen_world::World as GenWorld;
use worldgen_world::streaming::{self, Request, StreamParams};

#[derive(Resource)]
pub struct WorldRes {
    pub world: GenWorld,
}

#[derive(Resource, Clone)]
pub struct StreamConfig {
    pub params: StreamParams,
    pub budget: WorldBudget,
    /// Seconds of camera velocity used to request chunks ahead of time.
    pub lookahead_s: f32,
    /// Generating chunks not needed for this many frames are cancelled.
    pub cancel_after_frames: u64,
}

/// Marks the camera (or other entity) that drives streaming. Add [`crate::WorldAnchored`] too.
#[derive(Component)]
pub struct StreamFocus;

/// Root entity of a realised chunk.
#[derive(Component)]
pub struct ChunkRoot {
    pub id: ChunkId,
}

/// Generated data attached to a chunk root; renderers react to `Added<ChunkData>`.
#[derive(Component, Clone)]
pub struct ChunkData(pub Arc<GeneratedChunk>);

pub enum ChunkState {
    Generating {
        cancel: CancelToken,
        ticket: u64,
    },
    Ready(Arc<GeneratedChunk>),
    Resident {
        entity: Entity,
        data: Arc<GeneratedChunk>,
        shown: bool,
        /// In-flight regeneration after an invalidation (the old mesh stays visible meanwhile).
        regen: Option<Regen>,
        /// Regenerated data waiting to replace the visible chunk.
        pending: Option<Arc<GeneratedChunk>>,
    },
}

pub struct Regen {
    ticket: u64,
    cancel: CancelToken,
    started: Instant,
}

pub(crate) struct Entry {
    pub state: ChunkState,
    pub last_needed: u64,
}

type Key = (ChunkId, u64);

#[derive(Resource)]
pub struct ChunkStore {
    pub(crate) entries: HashMap<ChunkId, Entry>,
    pool: Mutex<JobPool<Key, GeneratedChunk>>,
    pub version: GenVersion,
    frame: u64,
    next_ticket: u64,
    /// Entities replaced by a regeneration; despawned a couple of frames later so the new
    /// chunk's meshes exist before the old ones disappear.
    retiring: Vec<(Entity, u64)>,
}

impl ChunkStore {
    pub(crate) fn new(threads: usize) -> Self {
        Self {
            entries: HashMap::new(),
            pool: Mutex::new(JobPool::new(threads)),
            version: GenVersion(1),
            frame: 0,
            next_ticket: 1,
            retiring: Vec::new(),
        }
    }

    pub fn state(&self, id: ChunkId) -> Option<&ChunkState> {
        self.entries.get(&id).map(|e| &e.state)
    }

    pub fn data(&self, id: ChunkId) -> Option<&Arc<GeneratedChunk>> {
        match &self.entries.get(&id)?.state {
            ChunkState::Ready(d) | ChunkState::Resident { data: d, .. } => Some(d),
            ChunkState::Generating { .. } => None,
        }
    }

    pub fn is_ready(&self, id: ChunkId) -> bool {
        self.data(id).is_some()
    }

    /// Regenerate every chunk intersecting `b` (after terrain edits). Resident chunks keep their
    /// current mesh until the replacement is ready; queued/in-flight work for the area is dropped.
    pub fn invalidate_region(&mut self, world: &GenWorld, b: &worldgen_terrain::Bounds) -> usize {
        let cfg = world.cfg();
        let ids: Vec<ChunkId> = self
            .entries
            .keys()
            .copied()
            .filter(|id| cfg.bounds(*id).intersects(b))
            .collect();
        let mut n = 0;
        for id in ids {
            n += 1;
            let ticket = self.next_ticket;
            let entry = self.entries.get_mut(&id).unwrap();
            match &mut entry.state {
                ChunkState::Generating { cancel, .. } => {
                    cancel.cancel();
                    self.entries.remove(&id);
                }
                ChunkState::Ready(_) => {
                    self.entries.remove(&id);
                }
                ChunkState::Resident { regen, pending, .. } => {
                    if let Some(r) = regen.take() {
                        r.cancel.cancel();
                    }
                    *pending = None;
                    self.next_ticket += 1;
                    let (w, version) = (world.clone(), self.version);
                    let cancel =
                        self.pool
                            .lock()
                            .unwrap()
                            .submit((id, ticket), version, -10.0, move |c| w.generate(id, c));
                    *regen = Some(Regen {
                        ticket,
                        cancel,
                        started: Instant::now(),
                    });
                }
            }
        }
        n
    }

    pub fn thread_count(&self) -> usize {
        self.pool.lock().unwrap().thread_count()
    }
}

#[derive(Resource, Default, Debug, Clone)]
pub struct StreamStats {
    pub frame: u64,
    pub generating: usize,
    pub ready: usize,
    pub resident: usize,
    pub shown: usize,
    pub visible_planned: usize,
    pub plan_complete: bool,
    pub jobs_done: u64,
    pub jobs_cancelled: u64,
    pub gen_ms_ema: f32,
    pub gen_ms_max: f32,
    pub collect_ms_last: f32,
    pub collect_ms_max: f32,
    pub realise_ms_last: f32,
    pub realise_ms_max: f32,
    pub chunks_realised_last: usize,
    pub regen_pending: usize,
    pub regen_done: u64,
    pub regen_latency_ms_last: f32,
    pub regen_latency_ms_max: f32,
    pub cpu_mb: f32,
}

#[derive(Resource, Default)]
pub(crate) struct Focus {
    pub pos: DVec3,
    pub vel: DVec3,
    pub valid: bool,
}

#[derive(Resource, Default)]
pub(crate) struct PlanState {
    pub visible: Vec<ChunkId>,
    pub requests: Vec<Request>,
}

/// Ask the streamer to regenerate every chunk intersecting a world rectangle (after edits).
#[derive(Message, Clone)]
pub struct InvalidateRegion(pub worldgen_terrain::Bounds);

pub(crate) fn apply_invalidations(
    mut msgs: MessageReader<InvalidateRegion>,
    world: Res<WorldRes>,
    mut store: ResMut<ChunkStore>,
) {
    for m in msgs.read() {
        store.invalidate_region(&world.world, &m.0);
    }
}

pub(crate) fn observe(
    time: Res<Time>,
    origin: Res<WorldOrigin>,
    cam: Query<&Transform, With<StreamFocus>>,
    mut focus: ResMut<Focus>,
    mut store: ResMut<ChunkStore>,
) {
    store.frame += 1;
    let Ok(t) = cam.single() else { return };
    let p = origin.to_world(t.translation);
    let dt = time.delta_secs_f64().max(1e-4);
    if focus.valid {
        let v = (p - focus.pos) / dt;
        focus.vel = focus.vel * 0.85 + v * 0.15;
    }
    focus.pos = p;
    focus.valid = true;
}

pub(crate) fn plan(
    cfg: Res<StreamConfig>,
    world: Res<WorldRes>,
    focus: Res<Focus>,
    mut store: ResMut<ChunkStore>,
    mut ps: ResMut<PlanState>,
) {
    if !focus.valid {
        return;
    }
    let ready = |id: ChunkId| store.is_ready(id);
    let p = &focus.pos;
    let cur = streaming::plan(world.world.cfg(), &cfg.params, (p.x, p.y, p.z), &ready);
    let mut requests = cur.requests;
    let speed = focus.vel.length();
    if speed > 5.0 {
        let q = *p + focus.vel * cfg.lookahead_s as f64;
        let pred = streaming::plan(
            world.world.cfg(),
            &cfg.params,
            (q.x, q.y.max(p.y), q.z),
            &ready,
        );
        let have: HashSet<ChunkId> = requests.iter().map(|r| r.id).collect();
        for mut r in pred.requests {
            if !have.contains(&r.id) {
                r.priority += 0.75;
                requests.push(r);
            }
        }
        requests.sort_by(|a, b| a.priority.total_cmp(&b.priority));
    }
    let frame = store.frame;
    for id in cur.visible.iter().chain(requests.iter().map(|r| &r.id)) {
        if let Some(e) = store.entries.get_mut(id) {
            e.last_needed = frame;
        }
    }
    ps.visible = cur.visible;
    ps.requests = requests;
}

pub(crate) fn dispatch(
    cfg: Res<StreamConfig>,
    world: Res<WorldRes>,
    ps: Res<PlanState>,
    mut store: ResMut<ChunkStore>,
) {
    let mut inflight = store
        .entries
        .values()
        .filter(|e| matches!(e.state, ChunkState::Generating { .. }))
        .count();
    let max = cfg.budget.max_inflight_jobs as usize;
    for r in &ps.requests {
        if inflight >= max {
            break;
        }
        if store.entries.contains_key(&r.id) {
            continue;
        }
        let ticket = store.next_ticket;
        store.next_ticket += 1;
        let (w, id, version, frame) = (world.world.clone(), r.id, store.version, store.frame);
        let cancel =
            store
                .pool
                .lock()
                .unwrap()
                .submit((id, ticket), version, r.priority, move |c| {
                    w.generate(id, c)
                });
        store.entries.insert(
            id,
            Entry {
                state: ChunkState::Generating { cancel, ticket },
                last_needed: frame,
            },
        );
        inflight += 1;
    }
}

pub(crate) fn collect(mut store: ResMut<ChunkStore>, mut stats: ResMut<StreamStats>) {
    let t0 = Instant::now();
    let version = store.version;
    let mut n = 0;
    loop {
        let r = store.pool.lock().unwrap().try_recv();
        let Some(r) = r else { break };
        let (id, ticket) = r.key;
        let is_regen = matches!(
            store.entries.get(&id),
            Some(Entry { state: ChunkState::Resident { regen: Some(rg), .. }, .. }) if rg.ticket == ticket
        );
        if is_regen {
            if let Some(chunk) = r.value
                && let Some(Entry {
                    state: ChunkState::Resident { regen, pending, .. },
                    ..
                }) = store.entries.get_mut(&id)
            {
                let ms = regen
                    .take()
                    .map_or(0.0, |rg| rg.started.elapsed().as_secs_f32() * 1000.0);
                *pending = Some(Arc::new(chunk));
                stats.regen_done += 1;
                stats.regen_latency_ms_last = ms;
                stats.regen_latency_ms_max = stats.regen_latency_ms_max.max(ms);
            }
            n += 1;
            continue;
        }
        let ok = r.version == version
            && matches!(store.entries.get(&id), Some(Entry { state: ChunkState::Generating { ticket: t, .. }, .. }) if *t == ticket);
        match (ok, r.value) {
            (true, Some(chunk)) => {
                stats.jobs_done += 1;
                stats.gen_ms_ema = if stats.gen_ms_ema == 0.0 {
                    r.run_ms
                } else {
                    stats.gen_ms_ema * 0.95 + r.run_ms * 0.05
                };
                stats.gen_ms_max = stats.gen_ms_max.max(r.run_ms);
                store.entries.get_mut(&id).unwrap().state = ChunkState::Ready(Arc::new(chunk));
            }
            (true, None) => {
                stats.jobs_cancelled += 1;
                store.entries.remove(&id);
            }
            _ => {}
        }
        n += 1;
        // keep the main thread cheap: stop when over 1 ms (the rest arrive next frame)
        if n >= 4 && t0.elapsed().as_secs_f32() > 0.001 {
            break;
        }
    }
    let ms = t0.elapsed().as_secs_f32() * 1000.0;
    stats.collect_ms_last = ms;
    stats.collect_ms_max = stats.collect_ms_max.max(ms);
}

pub(crate) fn realise(
    mut commands: Commands,
    cfg: Res<StreamConfig>,
    world: Res<WorldRes>,
    origin: Res<WorldOrigin>,
    ps: Res<PlanState>,
    mut store: ResMut<ChunkStore>,
    mut stats: ResMut<StreamStats>,
) {
    let t0 = Instant::now();
    let visible: HashSet<ChunkId> = ps.visible.iter().copied().collect();
    let mut bytes = 0usize;
    let mut realised = 0usize;
    for id in &ps.visible {
        let Some(e) = store.entries.get_mut(id) else {
            continue;
        };
        match &mut e.state {
            ChunkState::Ready(data) => {
                if realised > 0 && bytes as u64 >= cfg.budget.upload_bytes_per_frame {
                    continue;
                }
                bytes += data.approx_bytes();
                realised += 1;
                let b = world.world.cfg().bounds(*id);
                let pos = origin.to_render(DVec3::new(b.min_x, 0.0, b.min_z));
                let data = data.clone();
                let entity = commands
                    .spawn((
                        ChunkRoot { id: *id },
                        ChunkData(data.clone()),
                        Transform::from_translation(pos),
                        Visibility::Inherited,
                    ))
                    .id();
                e.state = ChunkState::Resident {
                    entity,
                    data,
                    shown: true,
                    regen: None,
                    pending: None,
                };
            }
            ChunkState::Resident { entity, shown, .. } if !*shown => {
                commands.entity(*entity).insert(Visibility::Inherited);
                *shown = true;
            }
            _ => {}
        }
    }
    // swap in regenerated chunks (budgeted like any other upload)
    let frame = store.frame;
    let mut retired: Vec<(Entity, u64)> = Vec::new();
    for (id, e) in store.entries.iter_mut() {
        if let ChunkState::Resident {
            entity,
            data,
            shown,
            pending,
            ..
        } = &mut e.state
            && pending.is_some()
            && (realised == 0 || (bytes as u64) < cfg.budget.upload_bytes_per_frame)
        {
            let new = pending.take().unwrap();
            bytes += new.approx_bytes();
            realised += 1;
            let b = world.world.cfg().bounds(*id);
            let pos = origin.to_render(DVec3::new(b.min_x, 0.0, b.min_z));
            let vis = if *shown {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            let ne = commands
                .spawn((
                    ChunkRoot { id: *id },
                    ChunkData(new.clone()),
                    Transform::from_translation(pos),
                    vis,
                ))
                .id();
            retired.push((*entity, frame + 2));
            *entity = ne;
            *data = new;
        }
    }
    store.retiring.extend(retired);
    let due: Vec<Entity> = store
        .retiring
        .iter()
        .filter(|(_, f)| *f <= frame)
        .map(|(e, _)| *e)
        .collect();
    for e in due {
        commands.entity(e).despawn();
    }
    store.retiring.retain(|(_, f)| *f > frame);
    let mut shown_n = 0;
    for (id, e) in store.entries.iter_mut() {
        if let ChunkState::Resident { entity, shown, .. } = &mut e.state {
            if !visible.contains(id) && *shown {
                commands.entity(*entity).insert(Visibility::Hidden);
                *shown = false;
            }
            shown_n += usize::from(*shown);
        }
    }
    let ms = t0.elapsed().as_secs_f32() * 1000.0;
    stats.realise_ms_last = ms;
    stats.realise_ms_max = stats.realise_ms_max.max(ms);
    stats.chunks_realised_last = realised;
    stats.shown = shown_n;
}

pub(crate) fn evict(
    mut commands: Commands,
    cfg: Res<StreamConfig>,
    ps: Res<PlanState>,
    mut store: ResMut<ChunkStore>,
    mut stats: ResMut<StreamStats>,
) {
    let frame = store.frame;
    // cancel generating chunks nobody has asked for in a while
    let stale: Vec<ChunkId> = store
        .entries
        .iter()
        .filter(|(_, e)| {
            matches!(e.state, ChunkState::Generating { .. })
                && frame.saturating_sub(e.last_needed) > cfg.cancel_after_frames
        })
        .map(|(id, _)| *id)
        .collect();
    for id in stale {
        if let Some(Entry {
            state: ChunkState::Generating { cancel, .. },
            ..
        }) = store.entries.remove(&id)
        {
            cancel.cancel();
        }
    }
    // LRU eviction of cached chunks beyond the budget (never anything currently visible)
    let max = cfg.budget.max_resident_chunks as usize;
    let live_bytes = |e: &Entry| match &e.state {
        ChunkState::Generating { .. } => 0,
        ChunkState::Ready(d) => d.live_bytes(),
        ChunkState::Resident { data, .. } => data.live_bytes(),
    };
    let total_bytes: usize = store.entries.values().map(live_bytes).sum();
    let over_bytes = total_bytes as u64 > cfg.budget.max_resident_bytes;
    if store.entries.len() > max || over_bytes {
        let visible: HashSet<ChunkId> = ps.visible.iter().copied().collect();
        let mut cands: Vec<(u64, ChunkId)> = store
            .entries
            .iter()
            .filter(|(id, e)| {
                !visible.contains(id) && !matches!(e.state, ChunkState::Generating { .. })
            })
            .map(|(id, e)| (e.last_needed, *id))
            .collect();
        cands.sort();
        let mut excess = store.entries.len().saturating_sub(max);
        let mut bytes_left = total_bytes as u64;
        for (_, id) in cands {
            if excess == 0 && bytes_left <= cfg.budget.max_resident_bytes {
                break;
            }
            if let Some(e) = store.entries.remove(&id) {
                bytes_left = bytes_left.saturating_sub(live_bytes(&e) as u64);
                excess = excess.saturating_sub(1);
                if let ChunkState::Resident { entity, .. } = e.state {
                    commands.entity(entity).despawn();
                }
            }
        }
    }
    let (mut g, mut r, mut res, mut bytes, mut regen_pending) = (0, 0, 0, 0usize, 0usize);
    for e in store.entries.values() {
        match &e.state {
            ChunkState::Generating { .. } => g += 1,
            ChunkState::Ready(d) => {
                r += 1;
                bytes += d.live_bytes();
            }
            ChunkState::Resident {
                data,
                regen,
                pending,
                ..
            } => {
                res += 1;
                bytes += data.live_bytes();
                regen_pending += usize::from(regen.is_some() || pending.is_some());
            }
        }
    }
    stats.frame = frame;
    stats.generating = g;
    stats.ready = r;
    stats.resident = res;
    stats.visible_planned = ps.visible.len();
    stats.plan_complete = !ps.visible.is_empty() && g == 0;
    stats.cpu_mb = bytes as f32 / (1024.0 * 1024.0);
    stats.regen_pending = regen_pending;
}
