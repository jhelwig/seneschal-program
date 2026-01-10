//! Traveller RPG-specific tools for Mongoose Traveller 2nd Edition.
//!
//! This module provides tools for working with Traveller game mechanics:
//! - UWP (Universal World Profile) parsing
//! - Jump fuel and time calculations
//! - Skill lookups

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Traveller-specific tools for mgt2e native support
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TravellerTool {
    /// Parse a UWP (Universal World Profile) string
    ParseUwp {
        uwp: String, // e.g., "A867949-C"
    },

    /// Calculate jump requirements
    JumpCalculation {
        distance_parsecs: u8,
        ship_jump_rating: u8,
        ship_tonnage: u32,
    },

    /// Look up a specific skill's description and usage
    SkillLookup {
        skill_name: String,
        speciality: Option<String>,
    },
}

/// Parsed UWP data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedUwp {
    pub raw: String,
    pub starport: char,
    pub starport_quality: String,
    pub size: u8,
    pub size_km: String,
    pub atmosphere: u8,
    pub atmosphere_type: String,
    pub hydrographics: u8,
    pub hydrographics_percent: String,
    pub population: u8,
    pub population_range: String,
    pub government: u8,
    pub government_type: String,
    pub law_level: u8,
    pub law_description: String,
    pub tech_level: u8,
}

impl TravellerTool {
    /// Execute a Traveller-specific tool
    pub fn execute(&self) -> Result<serde_json::Value, String> {
        match self {
            TravellerTool::ParseUwp { uwp } => parse_uwp(uwp),
            TravellerTool::JumpCalculation {
                distance_parsecs,
                ship_jump_rating,
                ship_tonnage,
            } => calculate_jump(*distance_parsecs, *ship_jump_rating, *ship_tonnage),
            TravellerTool::SkillLookup {
                skill_name,
                speciality,
            } => lookup_skill(skill_name, speciality.as_deref()),
        }
    }
}

/// Parse a UWP string into structured data
fn parse_uwp(uwp: &str) -> Result<serde_json::Value, String> {
    let uwp = uwp.trim().to_uppercase();

    // UWP format: Starport-Size-Atmo-Hydro-Pop-Gov-Law-TL (e.g., A867949-C)
    // Can be written as A867949-C or A 867 949-C
    let clean: String = uwp
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();

    if clean.len() < 8 {
        return Err(format!(
            "Invalid UWP format: {} (expected 8+ characters)",
            uwp
        ));
    }

    let chars: Vec<char> = clean.chars().collect();

    let starport = chars[0];
    let size = parse_hex_digit(chars[1]).ok_or_else(|| format!("Invalid size: {}", chars[1]))?;
    let atmosphere =
        parse_hex_digit(chars[2]).ok_or_else(|| format!("Invalid atmosphere: {}", chars[2]))?;
    let hydrographics =
        parse_hex_digit(chars[3]).ok_or_else(|| format!("Invalid hydrographics: {}", chars[3]))?;
    let population =
        parse_hex_digit(chars[4]).ok_or_else(|| format!("Invalid population: {}", chars[4]))?;
    let government =
        parse_hex_digit(chars[5]).ok_or_else(|| format!("Invalid government: {}", chars[5]))?;
    let law_level =
        parse_hex_digit(chars[6]).ok_or_else(|| format!("Invalid law level: {}", chars[6]))?;
    let tech_level = if chars.len() > 7 {
        parse_hex_digit(chars[7]).ok_or_else(|| format!("Invalid tech level: {}", chars[7]))?
    } else {
        0
    };

    let parsed = ParsedUwp {
        raw: uwp,
        starport,
        starport_quality: starport_quality(starport),
        size,
        size_km: size_description(size),
        atmosphere,
        atmosphere_type: atmosphere_description(atmosphere),
        hydrographics,
        hydrographics_percent: format!("{}%", hydrographics * 10),
        population,
        population_range: population_description(population),
        government,
        government_type: government_description(government),
        law_level,
        law_description: law_description(law_level),
        tech_level,
    };

    serde_json::to_value(parsed).map_err(|e| e.to_string())
}

fn parse_hex_digit(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        'A'..='F' => Some(c as u8 - b'A' + 10),
        'a'..='f' => Some(c as u8 - b'a' + 10),
        _ => None,
    }
}

fn starport_quality(c: char) -> String {
    match c {
        'A' => "Excellent - Refined fuel, shipyard (all), repairs".to_string(),
        'B' => "Good - Refined fuel, shipyard (spacecraft), repairs".to_string(),
        'C' => "Routine - Unrefined fuel, shipyard (small craft), repairs".to_string(),
        'D' => "Poor - Unrefined fuel, limited repairs".to_string(),
        'E' => "Frontier - No fuel, no repairs".to_string(),
        'X' => "No starport".to_string(),
        _ => format!("Unknown ({})", c),
    }
}

fn size_description(size: u8) -> String {
    match size {
        0 => "< 1,000 km (asteroid/small body)".to_string(),
        1 => "1,600 km".to_string(),
        2 => "3,200 km".to_string(),
        3 => "4,800 km".to_string(),
        4 => "6,400 km".to_string(),
        5 => "8,000 km".to_string(),
        6 => "9,600 km".to_string(),
        7 => "11,200 km".to_string(),
        8 => "12,800 km (Earth-sized)".to_string(),
        9 => "14,400 km".to_string(),
        10 => "16,000 km".to_string(),
        _ => format!("{},000+ km", (size as u32) * 1600 / 1000),
    }
}

fn atmosphere_description(atmo: u8) -> String {
    match atmo {
        0 => "Vacuum".to_string(),
        1 => "Trace".to_string(),
        2 => "Very Thin, Tainted".to_string(),
        3 => "Very Thin".to_string(),
        4 => "Thin, Tainted".to_string(),
        5 => "Thin".to_string(),
        6 => "Standard".to_string(),
        7 => "Standard, Tainted".to_string(),
        8 => "Dense".to_string(),
        9 => "Dense, Tainted".to_string(),
        10 => "Exotic".to_string(),
        11 => "Corrosive".to_string(),
        12 => "Insidious".to_string(),
        13 => "Dense, High".to_string(),
        14 => "Thin, Low".to_string(),
        15 => "Unusual".to_string(),
        _ => format!("Unknown ({})", atmo),
    }
}

fn population_description(pop: u8) -> String {
    match pop {
        0 => "None".to_string(),
        1 => "Tens (1-99)".to_string(),
        2 => "Hundreds (100-999)".to_string(),
        3 => "Thousands (1,000-9,999)".to_string(),
        4 => "Tens of thousands".to_string(),
        5 => "Hundreds of thousands".to_string(),
        6 => "Millions".to_string(),
        7 => "Tens of millions".to_string(),
        8 => "Hundreds of millions".to_string(),
        9 => "Billions".to_string(),
        10 => "Tens of billions".to_string(),
        11 => "Hundreds of billions".to_string(),
        12 => "Trillions".to_string(),
        _ => format!("10^{}", pop),
    }
}

fn government_description(gov: u8) -> String {
    match gov {
        0 => "None".to_string(),
        1 => "Company/Corporation".to_string(),
        2 => "Participating Democracy".to_string(),
        3 => "Self-Perpetuating Oligarchy".to_string(),
        4 => "Representative Democracy".to_string(),
        5 => "Feudal Technocracy".to_string(),
        6 => "Captive Government".to_string(),
        7 => "Balkanization".to_string(),
        8 => "Civil Service Bureaucracy".to_string(),
        9 => "Impersonal Bureaucracy".to_string(),
        10 => "Charismatic Dictator".to_string(),
        11 => "Non-Charismatic Leader".to_string(),
        12 => "Charismatic Oligarchy".to_string(),
        13 => "Religious Dictatorship".to_string(),
        14 => "Religious Autocracy".to_string(),
        15 => "Totalitarian Oligarchy".to_string(),
        _ => format!("Unknown ({})", gov),
    }
}

fn law_description(law: u8) -> String {
    match law {
        0 => "No restrictions".to_string(),
        1 => "Body pistols, explosives, poison gas prohibited".to_string(),
        2 => "Portable energy weapons prohibited".to_string(),
        3 => "Machine guns, automatic weapons prohibited".to_string(),
        4 => "Light assault weapons prohibited".to_string(),
        5 => "Personal concealable weapons prohibited".to_string(),
        6 => "All firearms except shotguns prohibited".to_string(),
        7 => "Shotguns prohibited".to_string(),
        8 => "Blade weapons controlled".to_string(),
        9 => "All weapons prohibited".to_string(),
        _ => format!("Extreme ({})", law),
    }
}

/// Calculate jump fuel and time requirements
fn calculate_jump(
    distance: u8,
    jump_rating: u8,
    tonnage: u32,
) -> Result<serde_json::Value, String> {
    if distance == 0 {
        return Err("Distance must be at least 1 parsec".to_string());
    }

    if distance > jump_rating {
        return Err(format!(
            "Cannot jump {} parsecs with Jump-{} drive",
            distance, jump_rating
        ));
    }

    // Jump fuel consumption: 10% of hull tonnage per parsec jumped (simplified)
    // Actual formula may vary by edition
    let fuel_per_parsec = tonnage as f64 * 0.1;
    let total_fuel = fuel_per_parsec * distance as f64;

    // Jump time is approximately 1 week (168 hours) regardless of distance
    let jump_time_hours = 168;

    let result = serde_json::json!({
        "distance_parsecs": distance,
        "jump_rating": jump_rating,
        "ship_tonnage": tonnage,
        "fuel_required_tons": total_fuel,
        "jump_time_hours": jump_time_hours,
        "jump_time_description": "Approximately 1 week",
        "notes": [
            "Jump fuel consumption is typically 10% of hull tonnage per parsec",
            "Actual consumption may vary by drive efficiency",
            "Jump takes approximately 1 week regardless of distance"
        ]
    });

    Ok(result)
}

/// Look up skill information
fn lookup_skill(skill_name: &str, speciality: Option<&str>) -> Result<serde_json::Value, String> {
    // Basic skill data - in a full implementation this would come from ingested rulebooks
    let skill_data = get_basic_skill_info(skill_name);

    let result = serde_json::json!({
        "skill": skill_name,
        "speciality": speciality,
        "description": skill_data.0,
        "characteristic": skill_data.1,
        "specialities": skill_data.2,
        "note": "For detailed rules, use document_search to find the skill in the Core Rulebook"
    });

    Ok(result)
}

fn get_basic_skill_info(skill: &str) -> (&'static str, &'static str, Vec<&'static str>) {
    let skill_lower = skill.to_lowercase();

    match skill_lower.as_str() {
        "admin" => (
            "Administration and bureaucratic tasks",
            "EDU or INT",
            vec![],
        ),
        "advocate" => ("Legal knowledge and courtroom skills", "EDU", vec![]),
        "animals" => (
            "Handling and training animals",
            "INT or DEX",
            vec!["Handling", "Training", "Veterinary"],
        ),
        "athletics" => (
            "Physical activities and sports",
            "DEX or STR or END",
            vec!["Dexterity", "Endurance", "Strength"],
        ),
        "art" => (
            "Creative and artistic endeavors",
            "INT or EDU",
            vec![
                "Performer",
                "Holography",
                "Instrument",
                "Visual Media",
                "Write",
            ],
        ),
        "astrogation" => ("Plotting courses through space", "EDU or INT", vec![]),
        "broker" => ("Trading and negotiating deals", "INT", vec![]),
        "carouse" => ("Socializing and drinking", "SOC", vec![]),
        "deception" => ("Lying and misdirection", "INT", vec![]),
        "diplomat" => ("Negotiation and etiquette", "SOC", vec![]),
        "drive" => (
            "Operating ground vehicles",
            "DEX",
            vec!["Hovercraft", "Mole", "Track", "Walker", "Wheel"],
        ),
        "electronics" => (
            "Operating electronic devices",
            "EDU or INT",
            vec!["Comms", "Computers", "Remote Ops", "Sensors"],
        ),
        "engineer" => (
            "Operating and maintaining ship systems",
            "EDU or INT",
            vec!["J-drive", "Life Support", "M-drive", "Power"],
        ),
        "explosives" => ("Using and defusing explosives", "EDU or INT", vec![]),
        "flyer" => (
            "Operating aircraft and grav vehicles",
            "DEX",
            vec!["Airship", "Grav", "Ornithopter", "Rotor", "Wing"],
        ),
        "gambler" => ("Games of chance", "INT", vec![]),
        "gun combat" => (
            "Using personal ranged weapons",
            "DEX",
            vec!["Archaic", "Energy", "Slug"],
        ),
        "gunner" => (
            "Operating ship-mounted weapons",
            "DEX",
            vec!["Capital", "Ortillery", "Screen", "Turret"],
        ),
        "heavy weapons" => (
            "Using heavy military weapons",
            "DEX or STR",
            vec!["Artillery", "Man Portable", "Vehicle"],
        ),
        "investigate" => ("Research and investigation", "INT or EDU", vec![]),
        "jack-of-all-trades" | "jack of all trades" => {
            ("Reduces unskilled penalties", "N/A", vec![])
        }
        "language" => (
            "Speaking and understanding languages",
            "EDU or INT",
            vec!["Anglic", "Vilani", "Zdetl", "Oynprith"],
        ),
        "leadership" => ("Commanding and inspiring others", "SOC", vec![]),
        "mechanic" => ("Repairing and maintaining equipment", "EDU or INT", vec![]),
        "medic" => ("Medical treatment and surgery", "EDU or INT", vec![]),
        "melee" => (
            "Close combat",
            "DEX or STR",
            vec!["Blade", "Bludgeon", "Natural", "Unarmed"],
        ),
        "navigation" => ("Finding routes on planets", "INT or EDU", vec![]),
        "persuade" => ("Convincing others", "SOC", vec![]),
        "pilot" => (
            "Operating spacecraft",
            "DEX",
            vec!["Capital Ships", "Small Craft", "Spacecraft"],
        ),
        "profession" => (
            "Career-specific skills",
            "INT or EDU",
            vec![
                "Belter",
                "Biologicals",
                "Civil Engineering",
                "Construction",
                "Hydroponics",
                "Polymers",
            ],
        ),
        "recon" => ("Scouting and observation", "INT", vec![]),
        "science" => (
            "Scientific knowledge",
            "EDU or INT",
            vec![
                "Archaeology",
                "Astronomy",
                "Biology",
                "Chemistry",
                "Cosmology",
                "Cybernetics",
                "Economics",
                "Genetics",
                "History",
                "Linguistics",
                "Philosophy",
                "Physics",
                "Planetology",
                "Psionicology",
                "Psychology",
                "Robotics",
                "Sophontology",
                "Xenology",
            ],
        ),
        "seafarer" => (
            "Operating watercraft",
            "DEX or INT",
            vec!["Ocean Ships", "Personal", "Sail", "Submarine"],
        ),
        "stealth" => ("Moving undetected", "DEX", vec![]),
        "steward" => ("Serving passengers", "SOC", vec![]),
        "streetwise" => ("Knowledge of criminal underworld", "INT", vec![]),
        "survival" => ("Living in hostile environments", "EDU or INT", vec![]),
        "tactics" => ("Military tactics", "INT or EDU", vec!["Military", "Naval"]),
        "vacc suit" => ("Operating in vacuum suits", "DEX or END", vec![]),
        _ => (
            "Unknown skill - search rulebooks for details",
            "Varies",
            vec![],
        ),
    }
}
