// PhoneBoost bilingual UI copy. French is the default (Building France competition).
// Canonical runtime state names stay unchanged internally (see STATE_LABELS: the
// canonical key is preserved and shown on hover); only user-facing labels translate.

export const STRINGS = {
  fr: {
    brand_sub: "Centre de contrôle",
    lang_label: "FR",
    lang_other: "EN",
    nav: {
      overview: "Aperçu",
      why: "Pourquoi",
      how: "Fonctionnement",
      ladder: "Contrôle & confiance",
      evidence: "Preuves",
      roadmap: "Feuille de route",
    },
    hero_kicker: "Calcul distant sécurisé · parc informatique",
    hero_headline_a: "Prolongez la durée de vie",
    hero_headline_b: "de votre parc informatique.",
    hero_para:
      "Utilisez intelligemment les ressources de smartphones Android déjà disponibles pour prolonger la durée de vie utile de vos PC Linux — sans prétendre transformer le téléphone en RAM ou CPU local.",
    hero_support:
      "Réduire les remplacements prématurés. Tirer davantage de valeur du parc existant.",
    cta_primary: "Découvrir PhoneBoost",
    cta_secondary: "Voir comment ça fonctionne",
    chip_live: "Live · indisponible",
    chip_evidence: "Preuves enregistrées · disponibles",
    audience_note:
      "Conçu pour les organisations — PME, associations, écoles, organismes publics et parcs informatiques — et également utile à toute personne souhaitant prolonger la vie d’un ordinateur existant.",

    why_kicker: "Le problème avant la technologie",
    why_title: "Pourquoi PhoneBoost ?",
    why_problem:
      "Les organisations remplacent souvent des ordinateurs encore fonctionnels parce qu’ils deviennent limités pour certaines charges de travail. PhoneBoost explore une autre approche : utiliser de façon sécurisée et explicite les ressources Android disponibles lorsque cela est réellement utile.",
    why_points: [
      {
        title: "Prolonger la vie utile des ordinateurs existants",
        desc: "Repousser le moment où une machine devient insuffisante pour certaines tâches.",
      },
      {
        title: "Retarder une partie du renouvellement matériel",
        desc: "Potentiellement différer certains remplacements, sans promesse chiffrée.",
      },
      {
        title: "Valoriser les appareils Android déjà possédés",
        desc: "Tirer davantage de valeur des smartphones que l’organisation détient déjà.",
      },
      {
        title: "Utiliser les ressources distantes seulement quand c’est pertinent",
        desc: "Recourir au distant uniquement lorsque la charge et les mesures le justifient.",
      },
      {
        title: "Rester local-first, sans dépendance cloud obligatoire",
        desc: "Le système fonctionne localement ; aucun service cloud n’est imposé.",
      },
      {
        title: "Protection Android sous contrôle de ResourceGuard",
        desc: "Température, batterie et mémoire du téléphone restent protégées par ResourceGuard.",
      },
    ],
    why_env:
      "Bénéfice environnemental : il découle de la réduction du remplacement prématuré des équipements.",

    how_kicker: "Comment ça fonctionne ?",
    how_title: "Un PC Linux, un lien local sécurisé, un smartphone Android",
    how_intro:
      "PhoneBoost est une coopération explicite entre deux machines distinctes. Le PC Linux orchestre les demandes ; le worker Android garde l’autorité finale sur ses propres ressources via ResourceGuard.",
    how_node_local: "PC Linux",
    how_node_local_sub: "x86-64 · orchestrateur",
    how_link: "Lien local sécurisé",
    how_link_sub: "Noise XX → IK · PBMUX · fail-closed",
    how_node_remote: "Smartphone Android",
    how_node_remote_sub: "ARM64 · worker de confiance",
    how_decision_title: "ResourceGuard · exécution distante · décision mesurée",
    how_decision_sub:
      "Le worker Android décide de l’admission ; l’exécution distante n’a lieu que sur décision mesurée.",
    how_separation:
      "Le téléphone agit comme un nœud distant séparé. Ses ressources ne sont jamais fusionnées avec celles de l’hôte Linux.",

    connects_label: "Comment ça se connecte",
    glance_label: "En un coup d’œil",
    glance: {
      availability: {
        label: "PhoneBoost est-il disponible ?",
        value: "Hors ligne dans le navigateur",
        note: "Nécessite un runtime phoneboostd local ; un navigateur hébergé ne peut pas l’atteindre.",
      },
      phone: {
        label: "Un téléphone contribue-t-il ?",
        value: "Aucun téléphone connecté",
        note: "Appairez un worker Android via le lien sécurisé pour contribuer.",
      },
      trust: {
        label: "État de confiance & contrôle",
        value: "Toutes les portes indisponibles",
        note: "Cinq portes d’autorité indépendantes — aucune satisfaite sans lien actif.",
      },
      evidence: {
        label: "Où sont les preuves ?",
        value: "Preuves disponibles",
        note: "Tests et builds vérifiés, issus du dépôt de la version.",
      },
    },

    capabilities_label: "Ce que le téléphone peut faire",
    capabilities: [
      {
        title: "Lien sécurisé",
        desc: "Canal authentifié et fail-closed entre le PC et le téléphone.",
        status: "Implémenté · non établi",
        tone: "unavailable",
      },
      {
        title: "Capacité distante",
        desc: "RemoteBuffer : objet distant volatile et borné. Protocole verrouillé ; chaîne de bout en bout encore non aboutie.",
        status: "Protocole verrouillé · e2e à venir",
        tone: "unavailable",
      },
      {
        title: "Calcul distant",
        desc: "Tâches distantes explicites, dont BLAKE3 auto-use, sous autorité du worker. Protocole verrouillé ; e2e à venir.",
        status: "Protocole verrouillé · e2e à venir",
        tone: "unavailable",
      },
      {
        title: "Preuves enregistrées",
        desc: "Tests et builds vérifiés, issus du dépôt de la version.",
        status: "Disponible",
        tone: "evidence",
      },
    ],

    limits_kicker: "Transparence",
    limits_title: "Ce que PhoneBoost ne fait pas",
    limits_sub:
      "Ces limites font partie du produit. PhoneBoost ne masque jamais ce qu’il n’est pas.",
    limits: [
      "Pas de RAM virtuelle ajoutée au PC.",
      "Pas de swap distant transparent.",
      "Pas de cœur CPU Android ajouté au scheduler Linux.",
      "Pas de promesse de gain sans mesure.",
    ],

    enable_title: "Activer PhoneBoost",
    enable_status: "Éteint · indisponible",
    enable_reason: "Runtime natif non joignable depuis ce navigateur hébergé.",
    enable_why: "Pourquoi je ne peux pas l’activer ?",
    enable_hide: "Masquer les détails",
    enable_no_fake: "Aucun état LIVE n’est jamais fabriqué ici.",

    ladder_kicker: "Modèle d’autorité",
    ladder_title: "Cinq portes indépendantes",
    ladder_sub:
      "Chaque porte est un contrôle d’autorité distinct — ni une séquence, ni une barre de progression. Un identifiant de pair est une identité seule ; il n’authentifie pas une session, n’accorde pas de bail et ne crée pas de capacité.",
    ladder_status: "Toutes indisponibles · aucun lien actif",
    gate_reason: "Aucun pont d’exécution en direct ; preuves enregistrées uniquement.",

    evidence_kicker: "Preuves enregistrées",
    evidence_title: "Preuves techniques auditées",
    evidence_sub:
      "Chaque carte est ancrée à un fichier présent dans le dépôt de la version. Rien ici n’est de la télémétrie en direct : ce sont des preuves vérifiées et enregistrées.",
    evidence_recorded: "Preuve enregistrée",
    evidence_open: "Ouvrir",
    evidence_fixtures: "fixtures",

    tech_label: "Détails techniques",
    tech_secure_link: "Détails du lien sécurisé",
    tech_endpoints: "Hôte local & worker distant",
    tech_remote_capability: "Capacité distante",
    tech_architecture: "Couches d’architecture",
    tech_repo_truth: "Vérité du dépôt",
    tech_security: "Sécurité",

    drawer_source: "source",
    drawer_structured: "Détail structuré",
    drawer_raw: "Preuve brute · expurgée à la source",
    drawer_loading: "Chargement des preuves…",

    roadmap_kicker: "Trajectoire",
    roadmap_title: "Aujourd’hui, ensuite, plus tard",
    roadmap_sub:
      "Une séparation nette entre ce qui est démontré aujourd’hui et ce qui reste à venir. Rien de « Ensuite » ou « Plus tard » n’est présenté comme implémenté.",
    roadmap_working: "Fonctionnel / démontré aujourd’hui",
    roadmap_next: "Ensuite",
    roadmap_future: "Plus tard",

    footer_release: "version",
    footer_master: "master",
    footer_repo: "Dépôt",
    footer_disclaimer:
      "PhoneBoost est une preuve de concept non destinée à la production, pour une coopération explicite de ressources distantes entre un hôte Linux x86-64 et un worker Android ARM64. Ce n’est pas une extension de RAM, ni du swap, ni une illusion de CPU, ni un service cloud. Ce centre de contrôle ne fabrique jamais d’état LIVE.",
    banner_poc: "Preuve de concept · non-production",
    banner_recorded: "Preuves enregistrées",
  },

  en: {
    brand_sub: "Control Center",
    lang_label: "EN",
    lang_other: "FR",
    nav: {
      overview: "Overview",
      why: "Why",
      how: "How it works",
      ladder: "Trust & control",
      evidence: "Evidence",
      roadmap: "Roadmap",
    },
    hero_kicker: "Secure remote compute · IT fleet",
    hero_headline_a: "Extend the useful life",
    hero_headline_b: "of your IT fleet.",
    hero_para:
      "Intelligently use resources from Android smartphones you already own to extend the usefulness of your Linux PCs — without pretending to turn the phone into local RAM or CPU.",
    hero_support:
      "Replace less hardware prematurely. Get more value from the devices you already own.",
    cta_primary: "Discover PhoneBoost",
    cta_secondary: "See how it works",
    chip_live: "Live · unavailable",
    chip_evidence: "Recorded evidence · available",
    audience_note:
      "Built for organizations — SMEs, associations, schools, public bodies and IT fleets — and also useful for anyone who wants to extend the life of an existing computer.",

    why_kicker: "The problem before the technology",
    why_title: "Why PhoneBoost?",
    why_problem:
      "Organizations often replace computers that are still functional because they become limited for certain workloads. PhoneBoost explores another approach: securely and explicitly using available Android resources when doing so is actually useful.",
    why_points: [
      {
        title: "Extend the useful life of existing computers",
        desc: "Push back the point where a machine becomes insufficient for certain tasks.",
      },
      {
        title: "Potentially delay part of the hardware renewal cycle",
        desc: "May defer some replacements — with no quantified promise.",
      },
      {
        title: "Make better use of Android devices already owned",
        desc: "Get more value from the smartphones the organization already has.",
      },
      {
        title: "Use remote resources only when it is relevant",
        desc: "Reach for remote resources only when the workload and measurements justify it.",
      },
      {
        title: "Stay local-first, no mandatory cloud dependency",
        desc: "The system runs locally; no cloud service is required.",
      },
      {
        title: "Android protection under ResourceGuard control",
        desc: "Phone thermal, battery and memory stay protected by ResourceGuard.",
      },
    ],
    why_env:
      "Environmental benefit: it comes from reducing premature hardware replacement.",

    how_kicker: "How it works",
    how_title: "A Linux PC, a secure local link, an Android smartphone",
    how_intro:
      "PhoneBoost is explicit cooperation between two separate machines. The Linux PC orchestrates requests; the Android worker keeps final authority over its own resources via ResourceGuard.",
    how_node_local: "Linux PC",
    how_node_local_sub: "x86-64 · orchestrator",
    how_link: "Secure local link",
    how_link_sub: "Noise XX → IK · PBMUX · fail-closed",
    how_node_remote: "Android smartphone",
    how_node_remote_sub: "ARM64 · trusted worker",
    how_decision_title: "ResourceGuard · remote execution · measured decision",
    how_decision_sub:
      "The Android worker decides admission; remote execution happens only on a measured decision.",
    how_separation:
      "The phone acts as a separate remote node. Its resources are never merged into the Linux host.",

    connects_label: "How it connects",
    glance_label: "At a glance",
    glance: {
      availability: {
        label: "Is PhoneBoost available?",
        value: "Offline in browser",
        note: "Needs a local phoneboostd runtime; a hosted browser cannot reach it.",
      },
      phone: {
        label: "Is a phone contributing?",
        value: "No phone connected",
        note: "Pair an Android worker over the secure link to contribute.",
      },
      trust: {
        label: "Trust & control state",
        value: "All gates unavailable",
        note: "Five independent authority gates — none satisfied without a live link.",
      },
      evidence: {
        label: "Where's the proof?",
        value: "Evidence available",
        note: "Verified tests and builds from the release repository.",
      },
    },

    capabilities_label: "What the phone can do",
    capabilities: [
      {
        title: "Secure link",
        desc: "Authenticated, fail-closed channel between PC and phone.",
        status: "Implemented · not established",
        tone: "unavailable",
      },
      {
        title: "Remote capacity",
        desc: "RemoteBuffer: bounded, volatile remote object. Wire protocol locked; end-to-end path still gated.",
        status: "Wire-locked · e2e roadmap",
        tone: "unavailable",
      },
      {
        title: "Remote compute",
        desc: "Explicit worker-authoritative jobs, incl. auto-use BLAKE3. Wire locked; end-to-end gated.",
        status: "Wire-locked · e2e roadmap",
        tone: "unavailable",
      },
      {
        title: "Recorded evidence",
        desc: "Verified tests and builds from the release repository.",
        status: "Available",
        tone: "evidence",
      },
    ],

    limits_kicker: "Transparency",
    limits_title: "What PhoneBoost does not do",
    limits_sub:
      "These limits are part of the product. PhoneBoost never hides what it is not.",
    limits: [
      "No virtual RAM added to the PC.",
      "No transparent remote swap.",
      "No Android CPU cores added to the Linux scheduler.",
      "No performance promise without measurement.",
    ],

    enable_title: "Enable PhoneBoost",
    enable_status: "Off · Unavailable",
    enable_reason: "Native runtime not reachable from this hosted browser.",
    enable_why: "Why can't I enable it?",
    enable_hide: "Hide details",
    enable_no_fake: "No LIVE state is ever fabricated here.",

    ladder_kicker: "Authority model",
    ladder_title: "Five independent gates",
    ladder_sub:
      "Each gate is a distinct authority check — not a sequence and not a progress bar. A peer ID is identity only; it does not authenticate a session, grant a lease, or create capacity.",
    ladder_status: "All unavailable · no live link",
    gate_reason: "No live runtime bridge; recorded evidence only.",

    evidence_kicker: "Recorded evidence",
    evidence_title: "Truth-audited technical proof",
    evidence_sub:
      "Every card is anchored to a checked-in file in the release repository. Nothing here is live telemetry — it is verified, recorded evidence.",
    evidence_recorded: "Recorded evidence",
    evidence_open: "Open",
    evidence_fixtures: "fixtures",

    tech_label: "Technical details",
    tech_secure_link: "Secure link details",
    tech_endpoints: "Local host & remote worker",
    tech_remote_capability: "Remote capability",
    tech_architecture: "Architecture layers",
    tech_repo_truth: "Repository truth",
    tech_security: "Security",

    drawer_source: "source",
    drawer_structured: "Structured detail",
    drawer_raw: "Raw evidence · redacted at source",
    drawer_loading: "Loading evidence…",

    roadmap_kicker: "Trajectory",
    roadmap_title: "Now, next, and future",
    roadmap_sub:
      "A deliberate separation between what is demonstrated today and what remains ahead. Nothing in Next or Future is presented as implemented.",
    roadmap_working: "Working / demonstrated now",
    roadmap_next: "Next",
    roadmap_future: "Future",

    footer_release: "release",
    footer_master: "master",
    footer_repo: "Repository",
    footer_disclaimer:
      "PhoneBoost is a non-production proof of concept for explicit remote resource cooperation between a Linux x86-64 host and an Android ARM64 worker. It is not RAM extension, not swap, not a CPU illusion, and not a cloud service. This Control Center never fabricates LIVE state.",
    banner_poc: "Non-production PoC",
    banner_recorded: "Recorded evidence",
  },
};

// Canonical runtime state name -> friendly bilingual label. Canonical key is kept
// (shown on hover) so technical transparency is preserved.
export const STATE_LABELS = {
  NOT_RUNNING_IN_HOSTED_BROWSER: { fr: "Non exécuté dans le navigateur", en: "Not running in browser" },
  "IMPLEMENTED · NOT_REACHABLE_FROM_BROWSER": { fr: "Implémenté · non joignable", en: "Implemented · not reachable" },
  NOT_CONNECTED: { fr: "Non connecté", en: "Not connected" },
  IMPLEMENTED_NOT_CONNECTED: { fr: "Implémenté · non connecté", en: "Implemented · not connected" },
  UNKNOWN_NO_SESSION: { fr: "Inconnu · aucune session", en: "Unknown · no session" },
  NOT_MEASURED_IN_BROWSER: { fr: "Non mesuré dans le navigateur", en: "Not measured in browser" },
  NOT_ESTABLISHED: { fr: "Non établi", en: "Not established" },
  NOT_AUTHENTICATED: { fr: "Non authentifié", en: "Not authenticated" },
  NOT_MEASURED: { fr: "Non mesuré", en: "Not measured" },
  UNAVAILABLE: { fr: "Indisponible", en: "Unavailable" },
  NO_LEASE: { fr: "Aucun bail", en: "No lease" },
  NO_RESERVATION: { fr: "Aucune réservation", en: "No reservation" },
  ROADMAP: { fr: "Feuille de route", en: "Roadmap" },
};

// Five gates: bilingual name + explanation. Canonical ids unchanged.
export const GATE_CONTENT = {
  paired: {
    name: { fr: "Appairé", en: "Paired" },
    explanation: {
      fr: "Confiance durable par clé statique après Noise XX, comparaison SAS, confirmation mutuelle et validation.",
      en: "Durable static-key trust after Noise XX, SAS comparison, mutual confirmation, and commit.",
    },
  },
  authenticated: {
    name: { fr: "Authentifié", en: "Authenticated" },
    explanation: {
      fr: "La session Noise en cours prouve le pair épinglé pour cette connexion.",
      en: "The current Noise session proves the pinned peer for this connection.",
    },
  },
  controller_lease: {
    name: { fr: "Bail contrôleur", en: "Controller lease" },
    explanation: {
      fr: "Un seul contrôleur authentifié détient un bail courant pour l’incarnation active du worker.",
      en: "One authenticated controller holds a current lease for the current worker incarnation.",
    },
  },
  resource_admissible: {
    name: { fr: "Ressource admissible", en: "Resource admissible" },
    explanation: {
      fr: "L’état de santé Android récent et la politique ResourceGuard autorisent une réservation précise.",
      en: "Fresh Android-local health and ResourceGuard policy permit a specific reservation.",
    },
  },
  provider_ready: {
    name: { fr: "Fournisseur prêt", en: "Provider ready" },
    explanation: {
      fr: "Un fournisseur concret a engagé des ressources et peut accepter l’opération demandée.",
      en: "A concrete provider has committed resources and can accept the requested operation.",
    },
  },
};

export function stateLabel(canonical, lang) {
  const s = (canonical || "").toUpperCase();
  if (s === "ROADMAP") return STATE_LABELS.ROADMAP[lang];
  const hit = STATE_LABELS[canonical];
  return hit ? hit[lang] : canonical;
}
