export const COPY = {
  fr: {
    language: "FR",
    alternateLanguage: "EN",
    controlCenter: "Centre de contrôle",
    navigation: {
      overview: "Aperçu",
      why: "Pourquoi",
      runtime: "État runtime",
      ladder: "Contrôle & confiance",
      evidence: "Preuves",
      roadmap: "Feuille de route",
    },
    labels: {
      liveFresh: "LIVE · état natif récent",
      liveUnavailable: "Preuves enregistrées · LIVE indisponible",
      recordedEvidence: "Preuves enregistrées",
      lockedFixture: "Fixture BLAKE3 verrouillée",
      remoteAvailable: "LIVE · distant disponible",
      remoteUnavailable: "LIVE · distant indisponible",
      unavailable: "Indisponible",
      primaryNavigation: "Navigation principale",
      languageSelector: "Choix de langue",
      close: "Fermer",
      runFixture: "Exécuter c10-abc-v1",
      running: "Chemin de production en cours…",
      lastAction: "Dernière action navigateur · résultat observé",
      runtimeUnavailable: "Aucun snapshot récent du daemon local.",
      boundaryDetails: "Détails de frontière",
      unknownGates: "Pourquoi certaines portes sont inconnues ?",
      boundaryCopy:
        "C12 expose des observations passives : un indice de découverte, un bail C07 et la dernière preuve d’admission/readiness. Elles expirent sans créer de trafic distant.",
      fixedAction: "Ce bouton ne peut appeler que l’action de production fixe c10-abc-v1.",
      computeUnavailable: "L’action de calcul de production est indisponible.",
      computeError: "L’action de calcul de production a échoué.",
    },
    hero: {
      kicker: "Calcul distant sécurisé · parc informatique",
      titleLead: "Prolongez la durée de vie",
      titleAccent: "de votre parc informatique.",
      body:
        "Utilisez explicitement les ressources Android déjà disponibles pour prolonger la vie utile de PC Linux — sans prétendre transformer le téléphone en RAM ou CPU local.",
      support: "Réduire les remplacements prématurés. Tirer davantage de valeur du parc existant.",
      primary: "Pourquoi PhoneBoost",
      secondary: "Voir l’état runtime",
    },
    why: {
      kicker: "Le problème avant la technologie",
      title: "Pourquoi PhoneBoost ?",
      body:
        "Les organisations remplacent parfois des ordinateurs encore fonctionnels lorsqu’ils deviennent limités pour certaines tâches. PhoneBoost explore une coopération distante, sécurisée et explicite lorsque les mesures la justifient.",
      points: [
        "Prolonge la vie utile des ordinateurs existants.",
        "Peut différer certains remplacements, sans promesse chiffrée.",
        "Valorise des smartphones Android déjà possédés.",
        "Reste local-first, sans dépendance cloud obligatoire.",
      ],
      limit: "Le bénéfice environnemental dépend de la réduction réelle des remplacements prématurés ; aucune performance ni capacité n’est promise.",
    },
    runtime: {
      kicker: "État observé",
      title: "Runtime et coopération distante",
      body: "Le navigateur présente un snapshot local récent ou des preuves enregistrées séparément. Le worker Android conserve l’autorité sur ses ressources.",
      daemon: "Daemon local",
      authenticated: "Session authentifiée",
      discovery: "Dernière observation de découverte",
      lease: "Bail contrôleur",
      latestAdmissionProof: "Dernière preuve fraîche d’admission/readiness",
      provider: "Provider BLAKE3",
      autoUse: "Auto-use",
      unknown: "UNKNOWN — non exposé par C12",
      topology: "PC Linux · lien local sécurisé · smartphone Android",
      separateNode: "Le téléphone est un nœud distant séparé ; ses ressources ne sont jamais fusionnées avec l’hôte Linux.",
    },
    gates: {
      kicker: "Modèle d’autorité",
      title: "Cinq portes indépendantes",
      body:
        "Chaque porte est un contrôle distinct : une identité ne prouve ni une session, ni un bail, ni une admission.",
      fresh: "Snapshot natif récent",
      unavailable: "Aucun bridge LIVE récent",
    },
    evidence: {
      kicker: "Preuves enregistrées",
      title: "Preuves techniques auditées",
      body: "Chaque carte est ancrée à une preuve enregistrée. Elle ne remplace jamais une télémétrie LIVE.",
      open: "Ouvrir",
      source: "source",
      loading: "Chargement des preuves…",
    },
    roadmap: {
      kicker: "Trajectoire",
      title: "Aujourd’hui et suite du travail",
      body: "Les éléments démontrés, les étapes suivantes et les futurs sujets restent séparés.",
      working: "Fonctionnel / démontré",
      next: "Ensuite",
      future: "Plus tard",
    },
    footer:
      "PhoneBoost est une preuve de concept non destinée à la production pour une coopération explicite entre un hôte Linux x86-64 et un worker Android ARM64. Ce n’est ni une extension de RAM, ni du swap, ni une illusion de CPU, ni un service cloud.",
  },
  en: {
    language: "EN",
    alternateLanguage: "FR",
    controlCenter: "Control Center",
    navigation: {
      overview: "Overview",
      why: "Why",
      runtime: "Runtime state",
      ladder: "Trust & control",
      evidence: "Evidence",
      roadmap: "Roadmap",
    },
    labels: {
      liveFresh: "LIVE · fresh native state",
      liveUnavailable: "Recorded evidence · LIVE unavailable",
      recordedEvidence: "Recorded evidence",
      lockedFixture: "Locked BLAKE3 fixture",
      remoteAvailable: "LIVE · remote available",
      remoteUnavailable: "LIVE · remote unavailable",
      unavailable: "Unavailable",
      primaryNavigation: "Primary navigation",
      languageSelector: "Language selector",
      close: "Close",
      runFixture: "Run c10-abc-v1",
      running: "Production path running…",
      lastAction: "Last browser action · observed result",
      runtimeUnavailable: "No fresh local daemon snapshot.",
      boundaryDetails: "Boundary details",
      unknownGates: "Why are some gates unknown?",
      boundaryCopy:
        "C12 exposes passive observations: a discovery hint, a C07 lease, and the latest admission/readiness proof. They expire without creating remote traffic.",
      fixedAction: "This button can only invoke the fixed c10-abc-v1 production action.",
      computeUnavailable: "The production compute action was unavailable.",
      computeError: "The production compute action failed.",
    },
    hero: {
      kicker: "Secure remote compute · IT fleet",
      titleLead: "Extend the useful life",
      titleAccent: "of your IT fleet.",
      body:
        "Explicitly use Android resources already available to extend the useful life of Linux PCs — without pretending that a phone becomes local RAM or CPU.",
      support: "Replace less hardware prematurely. Get more value from the fleet you already own.",
      primary: "Why PhoneBoost",
      secondary: "See runtime state",
    },
    why: {
      kicker: "The problem before the technology",
      title: "Why PhoneBoost?",
      body:
        "Organizations sometimes replace still-functional computers when they become limited for particular tasks. PhoneBoost explores secure, explicit remote cooperation when measurements justify it.",
      points: [
        "Extend the useful life of existing computers.",
        "May defer some replacements, with no quantified promise.",
        "Make better use of Android phones already owned.",
        "Stay local-first, with no mandatory cloud dependency.",
      ],
      limit: "Any environmental benefit depends on genuinely reducing premature replacements; no performance or capacity claim is made.",
    },
    runtime: {
      kicker: "Observed state",
      title: "Runtime and remote cooperation",
      body: "The browser presents a fresh local snapshot or separately labeled recorded evidence. The Android worker retains authority over its resources.",
      daemon: "Local daemon",
      authenticated: "Authenticated session",
      discovery: "Latest discovery observation",
      lease: "Controller lease",
      latestAdmissionProof: "Latest fresh admission/readiness proof",
      provider: "BLAKE3 provider",
      autoUse: "Auto-use",
      unknown: "UNKNOWN — not exposed by C12",
      topology: "Linux PC · secure local link · Android smartphone",
      separateNode: "The phone is a separate remote node; its resources are never merged into the Linux host.",
    },
    gates: {
      kicker: "Authority model",
      title: "Five independent gates",
      body:
        "Each gate is distinct: identity alone proves neither a session, a lease, nor admission.",
      fresh: "Fresh native snapshot",
      unavailable: "No fresh LIVE bridge",
    },
    evidence: {
      kicker: "Recorded evidence",
      title: "Truth-audited technical proof",
      body: "Every card is anchored to recorded evidence. It never substitutes for LIVE telemetry.",
      open: "Open",
      source: "source",
      loading: "Loading evidence…",
    },
    roadmap: {
      kicker: "Trajectory",
      title: "Today and the work ahead",
      body: "Demonstrated work, next steps, and future work remain separate.",
      working: "Working / demonstrated",
      next: "Next",
      future: "Future",
    },
    footer:
      "PhoneBoost is a non-production proof of concept for explicit cooperation between a Linux x86-64 host and an Android ARM64 worker. It is not RAM extension, swap, a CPU illusion, or a cloud service.",
  },
};

export const GATE_COPY = {
  paired: { fr: "Appairé", en: "Paired" },
  authenticated: { fr: "Authentifié", en: "Authenticated" },
  controller_lease: { fr: "Bail contrôleur", en: "Controller lease" },
  resource_admissible: {
    fr: "Dernière preuve d’admission/readiness",
    en: "Latest admission/readiness proof",
  },
  provider_ready: { fr: "Provider prêt", en: "Provider ready" },
};

const STATE_COPY = {
  ACTIVE: { fr: "Actif", en: "Active" },
  AUTHENTICATED: { fr: "Authentifié", en: "Authenticated" },
  AVAILABLE: { fr: "Disponible", en: "Available" },
  FRESH_HINT: { fr: "Indice récent", en: "Fresh hint" },
  NO_HINT: { fr: "Aucun indice", en: "No hint" },
  BACKEND_UNAVAILABLE: { fr: "Découverte indisponible", en: "Discovery unavailable" },
  EXPIRED: { fr: "Expiré", en: "Expired" },
  FRESH_PASS: { fr: "Dernière preuve réussie", en: "Latest proof passed" },
  FAILED: { fr: "Dernière preuve échouée", en: "Latest proof failed" },
  STALE: { fr: "Expiré", en: "Stale" },
  READY: { fr: "Prêt", en: "Ready" },
  REACHABLE: { fr: "Joignable", en: "Reachable" },
  UNAVAILABLE: { fr: "Indisponible", en: "Unavailable" },
  UNKNOWN: { fr: "Inconnu", en: "Unknown" },
  RECONNECTING: { fr: "Reconnexion", en: "Reconnecting" },
  NOT_CONFIGURED: { fr: "Non configuré", en: "Not configured" },
  REMOTE_SUCCESS: { fr: "Succès distant", en: "Remote success" },
  LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE: {
    fr: "Fallback local après indisponibilité distante",
    en: "Local fallback after remote unavailable",
  },
  LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE: {
    fr: "Fallback local après résultat distant ambigu",
    en: "Local fallback after ambiguous remote",
  },
  ROADMAP: { fr: "Feuille de route", en: "Roadmap" },
};

export function copyFor(language) {
  return COPY[language] || COPY.fr;
}

export function stateLabel(state, language) {
  const canonical = typeof state === "string" ? state.toUpperCase() : "UNAVAILABLE";
  return STATE_COPY[canonical]?.[language] || state || STATE_COPY.UNAVAILABLE[language];
}
