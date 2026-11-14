# Reprise après qualification logicielle Android health

Lire [le rapport health](../../docs/architecture/ANDROID_HEALTH_SOURCE_V0_1.md) et [les preuves logicielles](../../docs/evidence/android-health-source-v0-1.json). `scripts/project-pipeline status` donne la branche et le HEAD courant. Vérifier les Actions et CodeQL du même SHA publié.

NEXT_ACTION : Valider sur appareil la provenance thermique et la cohérence des lectures/fraîcheur de la source Android health avant de reprendre le spike CPU multicœur, puis la RAM distante.

La correction part du master convergé `251306e825db79173f0bdeb97d1823d08d894ca7`, contenant #30, #27, #28 et #29. Le téléphone était indisponible : aucune nouvelle preuve physique. Les fixtures Kotlin/Rust et la vraie liaison JNI sur hôte ne qualifient pas le HAL OEM.

Le worktree historique reste intact à `9daae7fb496da2f903212d7c811357901f4b2aee`. Le collecteur quarantiné reste exclu. Ne pas relancer WebP, CAPACITY, THROUGHPUT, audit SoC ou étude below-OS ; ne pas ouvrir multicœur/RAM/GPU/NPU ni nettoyer les branches historiques.
