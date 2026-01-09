use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Access levels aligned with FVTT user roles
/// Values correspond to minimum required role to access
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema, Default,
)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum AccessLevel {
    Player = 1,    // Anyone with at least Player role
    Trusted = 2,   // Trusted players and above
    Assistant = 3, // Assistant GMs (may need scenario prep materials)
    #[default]
    GmOnly = 4, // Full GM only
}

impl AccessLevel {
    /// Check if this access level is accessible by a user with the given role
    pub fn accessible_by(&self, user_role: u8) -> bool {
        user_role >= *self as u8
    }

    /// Convert from u8, defaulting to GmOnly for invalid values
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => AccessLevel::Player,
            2 => AccessLevel::Trusted,
            3 => AccessLevel::Assistant,
            _ => AccessLevel::GmOnly,
        }
    }
}

/// Tag matching strategy for search filters
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TagMatch {
    #[default]
    Any, // Any of the specified tags
    All, // All of the specified tags
}

/// Search filters
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SearchFilters {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub tags_match: TagMatch,
}

/// Tool call from the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub tool: String,
    pub args: serde_json::Value,
}

/// Tool result to return to the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    #[serde(flatten)]
    pub outcome: ToolOutcome,
}

/// Tool execution outcome
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolOutcome {
    Success { result: serde_json::Value },
    Error { error: String },
}

impl ToolResult {
    pub fn success(tool_call_id: String, result: serde_json::Value) -> Self {
        Self {
            tool_call_id,
            outcome: ToolOutcome::Success { result },
        }
    }

    pub fn error(tool_call_id: String, error: String) -> Self {
        Self {
            tool_call_id,
            outcome: ToolOutcome::Error { error },
        }
    }
}

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

/// Tool definition for Ollama's tool format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OllamaFunctionDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaFunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Get tool definitions in Ollama's format
pub fn get_ollama_tool_definitions() -> Vec<OllamaToolDefinition> {
    vec![
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "document_search".to_string(),
                description: "Search game documents (rulebooks, scenarios) for information. Returns relevant text chunks.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query"
                        },
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional tags to filter results"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results (default 10)"
                        }
                    },
                    "required": ["query"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "document_get".to_string(),
                description: "Get a specific document or page by ID.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "document_id": {
                            "type": "string",
                            "description": "The document ID"
                        },
                        "page": {
                            "type": "integer",
                            "description": "Optional specific page number"
                        }
                    },
                    "required": ["document_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "document_list".to_string(),
                description: "List all available documents (rulebooks, scenarios) with their IDs and titles.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional tags to filter documents"
                        }
                    }
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "document_find".to_string(),
                description: "Find documents by title (case-insensitive partial match). Returns document IDs and metadata.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "The document title to search for (partial match)"
                        }
                    },
                    "required": ["title"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "image_list".to_string(),
                description: "List images from a document. Use document_find first to get the document ID, then use this to get images from specific pages.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "document_id": {
                            "type": "string",
                            "description": "The document ID"
                        },
                        "page": {
                            "type": "integer",
                            "description": "Optional: filter to images from this specific page number"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum images to return (default 20)"
                        }
                    },
                    "required": ["document_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "image_search".to_string(),
                description: "Search for images by description using semantic similarity. Good for finding maps, portraits, deck plans, etc.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Description of the image to find"
                        },
                        "document_id": {
                            "type": "string",
                            "description": "Optional: limit search to a specific document"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum results (default 10)"
                        }
                    },
                    "required": ["query"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "image_get".to_string(),
                description: "Get detailed information about a specific image by its ID.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "image_id": {
                            "type": "string",
                            "description": "The image ID"
                        }
                    },
                    "required": ["image_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "image_deliver".to_string(),
                description: "Copy an image to the Foundry VTT assets directory so it can be used in scenes, actors, etc. Returns the FVTT path to use.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "image_id": {
                            "type": "string",
                            "description": "The image ID to deliver"
                        },
                        "target_path": {
                            "type": "string",
                            "description": "Optional: custom path within FVTT assets (default: auto-generated from document title and page)"
                        }
                    },
                    "required": ["image_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "fvtt_read".to_string(),
                description: "Read a Foundry VTT document (Actor, Item, etc.). Respects user permissions.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "document_type": {
                            "type": "string",
                            "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                            "description": "The type of FVTT document"
                        },
                        "document_id": {
                            "type": "string",
                            "description": "The document ID"
                        }
                    },
                    "required": ["document_type", "document_id"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "fvtt_write".to_string(),
                description: "Create or modify a Foundry VTT document. Respects user permissions.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "document_type": {
                            "type": "string",
                            "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                            "description": "The type of FVTT document"
                        },
                        "operation": {
                            "type": "string",
                            "enum": ["create", "update", "delete"],
                            "description": "The operation to perform"
                        },
                        "data": {
                            "type": "object",
                            "description": "The document data"
                        }
                    },
                    "required": ["document_type", "operation", "data"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "fvtt_query".to_string(),
                description: "Query Foundry VTT documents with filters.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "document_type": {
                            "type": "string",
                            "enum": ["actor", "item", "journal_entry", "scene", "rollable_table"],
                            "description": "The type of FVTT document to query"
                        },
                        "filters": {
                            "type": "object",
                            "description": "Query filters (e.g., {name: 'Marcus'})"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum results (default 20)"
                        }
                    },
                    "required": ["document_type"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "dice_roll".to_string(),
                description: "Roll dice using FVTT's dice system. Results are logged to the game.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formula": {
                            "type": "string",
                            "description": "Dice formula (e.g., '2d6+2', '1d20')"
                        },
                        "label": {
                            "type": "string",
                            "description": "Optional label for the roll"
                        }
                    },
                    "required": ["formula"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "system_schema".to_string(),
                description: "Get the game system's schema for actors and items.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "document_type": {
                            "type": "string",
                            "enum": ["actor", "item"],
                            "description": "Optional: get schema for specific document type"
                        }
                    }
                }),
            },
        },
        // Traveller-specific tools
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "traveller_uwp_parse".to_string(),
                description: "Parse a Traveller UWP (Universal World Profile) string into detailed world data.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "uwp": {
                            "type": "string",
                            "description": "UWP string (e.g., 'A867949-C')"
                        }
                    },
                    "required": ["uwp"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "traveller_jump_calc".to_string(),
                description: "Calculate jump drive fuel requirements and time.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "distance_parsecs": {
                            "type": "integer",
                            "description": "Distance in parsecs"
                        },
                        "ship_jump_rating": {
                            "type": "integer",
                            "description": "Ship's jump drive rating (1-6)"
                        },
                        "ship_tonnage": {
                            "type": "integer",
                            "description": "Ship's total tonnage"
                        }
                    },
                    "required": ["distance_parsecs", "ship_jump_rating", "ship_tonnage"]
                }),
            },
        },
        OllamaToolDefinition {
            tool_type: "function".to_string(),
            function: OllamaFunctionDefinition {
                name: "traveller_skill_lookup".to_string(),
                description: "Look up a Traveller skill's description, characteristic, and specialities.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "skill_name": {
                            "type": "string",
                            "description": "Name of the skill"
                        },
                        "speciality": {
                            "type": "string",
                            "description": "Optional speciality"
                        }
                    },
                    "required": ["skill_name"]
                }),
            },
        },
    ]
}

/// Classify whether a tool is internal (backend-only) or external (requires client)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolLocation {
    Internal,
    External,
}

pub fn classify_tool(tool_name: &str) -> ToolLocation {
    match tool_name {
        "document_search"
        | "document_get"
        | "document_list"
        | "document_find"
        | "image_list"
        | "image_search"
        | "image_get"
        | "image_deliver"
        | "system_schema"
        | "traveller_uwp_parse"
        | "traveller_jump_calc"
        | "traveller_skill_lookup" => ToolLocation::Internal,
        "fvtt_read" | "fvtt_write" | "fvtt_query" | "dice_roll" => ToolLocation::External,
        _ => ToolLocation::External, // Unknown tools go to client for safety
    }
}
