/**
 * Mongoose Traveller 2nd Edition (MGT2E) specific enhancements
 */

/**
 * MGT2E specific schema and capability enhancements
 */
export const MGT2E_ENHANCEMENTS = {
  actorTypes: {
    traveller: {
      description: "Player character or NPC with full characteristics",
      characteristics: ["strength", "dexterity", "endurance", "intellect", "education", "social"],
      skillSystem: "Skills are embedded Items with value and optional speciality",
    },
    npc: {
      description: "Simplified NPC without full career history",
    },
    creature: {
      description: "Animal or alien creature with instinct-based behavior",
    },
    spacecraft: {
      description: "Starship with tonnage, jump rating, and crew positions",
    },
    vehicle: {
      description: "Ground or air vehicle",
    },
    world: {
      description: "Planet with UWP (Universal World Profile) data",
    },
  },
  itemTypes: {
    weapon: { key_fields: ["damage", "range", "traits", "tl"] },
    armour: { key_fields: ["protection", "tl", "radiation"] },
    skill: { key_fields: ["value", "speciality"] },
    term: { key_fields: ["career", "assignment", "rank"] },
  },
  uwpFormat: "Starport-Size-Atmo-Hydro-Pop-Gov-Law-TL (e.g., A867949-C)",
};
