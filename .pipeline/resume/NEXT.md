# Reprise après qualification physique Android health

Lire [le rapport health](../../docs/architecture/ANDROID_HEALTH_SOURCE_V0_1.md) et [les preuves logicielles et physiques](../../docs/evidence/android-health-source-v0-1.json). `scripts/project-pipeline status` donne la branche et le HEAD courant. Le jalon de publication est : Project pipeline et CodeQL verts sur le SHA final de #31, puis fusion et relevé du SHA de `master`.

NEXT_ACTION après publication validée de #31 : reprendre le spike CPU multicœur sous ResourceGuard, avec instrumentation physique, puis ouvrir le chantier RAM distante.

La correction part du master convergé `251306e825db79173f0bdeb97d1823d08d894ca7`, contenant #30, #27, #28 et #29. Le Samsung SM-A155F / Android 16 a permis de vérifier sur un APK identique à celui installé : HAL thermique prêt sans override, cohérence mémoire/batterie/charge/Battery Saver/thermique, et refus périmé avant lecture après une pause contrôlée écran éteint, suivi d'une mesure fraîche. La courte séquence ne prouve pas une veille profonde ; les tests déterministes couvrent les entrées absentes ou incohérentes non produites naturellement.

Le worktree historique reste intact à `9daae7fb496da2f903212d7c811357901f4b2aee`. Le collecteur quarantiné reste exclu. Ne pas relancer WebP, CAPACITY, THROUGHPUT, audit SoC ou étude below-OS ; ne pas ouvrir multicœur/RAM/GPU/NPU avant la fusion de #31 ni nettoyer les branches historiques.
