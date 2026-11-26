pub const DEFAULT: &str = "default";

pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub swatch: [&'static str; 2],
}

pub const PRESETS: &[Preset] = &[
    Preset {
        id: DEFAULT,
        name: "Default",
        swatch: ["#0e1118", "#dce0e5"],
    },
    Preset {
        id: "starry-columns",
        name: "Starry Columns",
        swatch: ["#071836", "#1d4f8c"],
    },
    Preset {
        id: "pantheon-rain",
        name: "Pantheon",
        swatch: ["#163875", "#4c86d6"],
    },
    Preset {
        id: "aqueduct-canyon",
        name: "Aqueduct",
        swatch: ["#23262b", "#8a8f96"],
    },
    Preset {
        id: "vaulted-sanctum",
        name: "Sanctum",
        swatch: ["#1c0c24", "#7b538a"],
    },
    Preset {
        id: "marble-hall",
        name: "Marble Hall",
        swatch: ["#efe9dc", "#c7b99e"],
    },
    Preset {
        id: "athena-clouds",
        name: "Olympus",
        swatch: ["#7c8a9e", "#dce3ec"],
    },
    Preset {
        id: "snowy-forum",
        name: "Winter Forum",
        swatch: ["#6a5e78", "#cfc6db"],
    },
    Preset {
        id: "misty-arch",
        name: "Triumphal Arch",
        swatch: ["#535f9e", "#c9cff0"],
    },
];

pub fn normalized(id: &str) -> &'static str {
    PRESETS
        .iter()
        .find(|preset| preset.id == id)
        .map_or(DEFAULT, |preset| preset.id)
}

pub fn bytes(id: &str, dark: bool) -> &'static [u8] {
    match id {
        "starry-columns" => include_bytes!("../../assets/home/backgrounds/starry-columns.jpg"),
        "pantheon-rain" => include_bytes!("../../assets/home/backgrounds/pantheon-rain.jpg"),
        "aqueduct-canyon" => include_bytes!("../../assets/home/backgrounds/aqueduct-canyon.jpg"),
        "vaulted-sanctum" => include_bytes!("../../assets/home/backgrounds/vaulted-sanctum.jpg"),
        "marble-hall" => include_bytes!("../../assets/home/backgrounds/marble-hall.jpg"),
        "athena-clouds" => include_bytes!("../../assets/home/backgrounds/athena-clouds.jpg"),
        "snowy-forum" => include_bytes!("../../assets/home/backgrounds/snowy-forum.jpg"),
        "misty-arch" => include_bytes!("../../assets/home/backgrounds/misty-arch.jpg"),
        _ if dark => include_bytes!("../../assets/black-hole-ascii.png"),
        _ => include_bytes!("../../assets/clouds-sunlit-ascii.png"),
    }
}
