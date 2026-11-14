//! CGA grammars. Each style is a text template; `{...}` placeholders are filled per regional style.
//! Randomness inside a grammar is seeded per lot, so a building is a pure function of its lot.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Region {
    /// Brick/stucco walls, tile/slate gable roofs.
    Temperate,
    /// Timber boards, steep dark roofs.
    Cold,
    /// Pale stucco, low pitched or flat roofs.
    Dry,
}

impl Region {
    pub fn from_climate(temp_c: f32, precip_mm: f32) -> Self {
        if temp_c < 6.5 {
            Region::Cold
        } else if precip_mm < 520.0 {
            Region::Dry
        } else {
            Region::Temperate
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    House,
    Shop,
    Tower,
    Barn,
}

struct Palette {
    walls: &'static str,
    roofs: &'static str,
    pitch: u32,
}

fn palette(r: Region) -> Palette {
    match r {
        Region::Temperate => Palette {
            walls: "55% BrickWall | 45% StuccoWall",
            roofs: "50% TileRoof | 35% SlateRoof | 15% ShingleRoof",
            pitch: 32,
        },
        Region::Cold => Palette {
            walls: "60% TimberWall | 40% DarkTimberWall",
            roofs: "70% ShingleRoof | 30% SlateRoof",
            pitch: 42,
        },
        Region::Dry => Palette {
            walls: "70% AdobeWall | 30% StuccoWall",
            roofs: "75% TerracottaRoof | 25% TileRoof",
            pitch: 16,
        },
    }
}

const COMMON: &str = r#"
BrickWall --> Extrude(0.22) Mat("Brick") I("Wall")
StuccoWall --> Extrude(0.22) Mat("Stucco") I("Wall")
TimberWall --> Extrude(0.22) Mat("Wood") I("Wall")
DarkTimberWall --> Extrude(0.22) Mat("WoodDark") I("Wall")
AdobeWall --> Extrude(0.22) Mat("Adobe") I("Wall")
ConcreteWall --> Extrude(0.22) Mat("Concrete") I("Wall")
TileRoof --> Mat("Tile") I("RoofTile")
SlateRoof --> Mat("Slate") I("RoofTile")
ShingleRoof --> Mat("Shingle") I("RoofTile")
TerracottaRoof --> Mat("Terracotta") I("RoofTile")
GlassPiece --> Extrude(0.07) Mat("Glass") I("Pane")
DoorPiece --> Extrude(0.12) Mat("Door") I("Door")
Frame --> Extrude(0.12) Mat("Trim") I("Trim")
"#;

/// Build the grammar text for a building kind in a regional style.
pub fn grammar(kind: Kind, region: Region, tall: (u32, u32)) -> String {
    let p = palette(region);
    let (walls, roofs, pitch) = (p.walls, p.roofs, p.pitch);
    let body = match kind {
        Kind::House => format!(
            r#"
Lot --> 55% Split(X) {{ ~7: HouseMass | ~3: Outbuilding }} | 45% HouseMass
HouseMass --> Split(Z) {{ ~1: Main | 3: Yard }}
Outbuilding --> Split(Z) {{ ~1: Shed | 3: Yard }}
Yard --> NIL
Shed --> Extrude(3.0) Split(Y) {{ 2.4: ShedWalls | 0.6: ShedRoof }}
ShedWalls --> Comp(Faces) {{ Side: TimberWall }}
ShedRoof --> Roof(Shed, 12) {{ Slope: ShingleRoof }}
Main --> 45% Extrude(rand(5.2, 6.4)) Split(Y) {{ 3.0: Ground | ~1: Top }} | 55% Extrude(rand(8.0, 9.4)) Split(Y) {{ 3.0: Ground | 2.9: Upper | ~1: Top }}
Ground --> Comp(Faces) {{ Front: FrontFacade | Back: SideFacade | Left: SideFacade | Right: SideFacade }}
Upper --> Comp(Faces) {{ Side: SideFacade }}
Top --> Roof(Gable, {pitch}) {{ Slope: Roofing | GableEnd: Gable }}
FrontFacade --> Split(X) {{ ~1: Wall | 1.1: Door | ~1: Wall | 1.5: Win | ~1: Wall }}
SideFacade --> Repeat(X, rand(3.0, 4.4)) {{ Bay }}
Bay --> Split(X) {{ ~1: Wall | rand(1.0, 1.6): Win | ~1: Wall }}
Win --> Split(Y) {{ 0.9: Wall | 1.3: GlassPiece | ~1: Wall }}
Door --> Split(Y) {{ 2.1: DoorPiece | ~1: Wall }}
Wall --> Pick("wall") {{ {walls} }}
Roofing --> Pick("roof") {{ {roofs} }}
Gable --> Pick("wall") {{ {walls} }}
"#
        ),
        Kind::Shop => format!(
            r#"
Lot --> Extrude(rand(9.5, 13.5)) Split(Y) {{ 4.0: ShopLevel | ~1: Floors | 2.4: Top }}
ShopLevel --> Comp(Faces) {{ Front: ShopFront | Back: Wall | Left: Wall | Right: Wall }}
ShopFront --> Split(X) {{ 0.5: Wall | ~1: Display | 1.2: Door | 0.5: Wall }}
Display --> Split(Y) {{ 0.7: Wall | 2.6: GlassPiece | ~1: Frame }}
Door --> Split(Y) {{ 2.2: DoorPiece | ~1: Wall }}
Floors --> Comp(Faces) {{ Front: UpperFront | Side: Wall }}
UpperFront --> Repeat(X, 3.2) {{ UpperBay }}
UpperBay --> Split(X) {{ ~1: Wall | 1.2: UpperWin | ~1: Wall }}
UpperWin --> Split(Y) {{ 0.9: Wall | 1.5: GlassPiece | ~1: Wall }}
Top --> Roof(Gable, {pitch}) {{ Slope: Roofing | GableEnd: Gable }}
Wall --> Pick("wall") {{ {walls} }}
Roofing --> Pick("roof") {{ {roofs} }}
Gable --> Pick("wall") {{ {walls} }}
"#
        ),
        Kind::Tower => {
            let (lo, hi) = tall;
            format!(
                r#"
Lot --> Extrude(rand({lo}, {hi})) Split(Y) {{ 4.5: Podium | ~1: Shaft | 1.4: Crown }}
Podium --> Comp(Faces) {{ Front: PodiumFront | Side: Wall }}
PodiumFront --> Split(X) {{ 0.8: Wall | ~1: PodiumGlass | 1.6: Door | 0.8: Wall }}
PodiumGlass --> Split(Y) {{ 0.6: Wall | 3.4: GlassPiece | ~1: Frame }}
Door --> Split(Y) {{ 2.6: DoorPiece | ~1: Wall }}
Shaft --> Comp(Faces) {{ Side: Curtain }}
Curtain --> Repeat(Y, 3.6) {{ Floor }}
Floor --> Split(Y) {{ 1.0: ConcreteWall | ~1: GlassPiece }}
Crown --> Comp(Faces) {{ Side: ConcreteWall | Top: ConcreteWall }}
Wall --> ConcreteWall
"#
            )
        }
        Kind::Barn => format!(
            r#"
Lot --> Extrude(5.0) Split(Y) {{ 3.6: BarnWalls | ~1: BarnRoof }}
BarnWalls --> Comp(Faces) {{ Front: BarnFront | Back: Wall | Left: Wall | Right: Wall }}
BarnFront --> Split(X) {{ ~1: Wall | ~3.2: BigDoor | ~1: Wall }}
BigDoor --> Split(Y) {{ 3.0: DoorPiece | ~1: Wall }}
BarnRoof --> Roof(Gable, {pitch}) {{ Slope: Roofing | GableEnd: Wall }}
Wall --> Pick("wall") {{ 60% TimberWall | 40% DarkTimberWall }}
Roofing --> Pick("roof") {{ 50% ShingleRoof | 50% SlateRoof }}
"#
        ),
    };
    format!("{body}{COMMON}")
}
