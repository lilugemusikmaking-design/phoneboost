# Reprise : diagnostic C12 physiquement terminé

Branche : `spike/c12-end-to-end-timing-v0-1`. Base master `524d0065f5609919c61e37c242771ee3bf61eb9b`.
HEAD exact à résoudre par `scripts/project-pipeline status` / GitHub.

[Résultats](../../docs/architecture/C12_END_TO_END_TIMING_RESULTS_V0_1.md) et
[preuves](../../docs/evidence/c12-end-to-end-timing-v0-1.json) : 3 runs complets
64 MiB distants exacts. Médiane end-to-end 26,502 s ; chiffrement Noise sur hôte
dev non optimisé 92,12 % ; compute Android environ 51 ms. Pas d'optimisation.
Le plan initial est exécuté, ne pas refaire les campagnes CPU ou le diagnostic.

Battery Saver ON, profil 1, collecte OFF, app/daemon/forward expérimentaux arrêtés,
produit inchangé. Les preuves multicœur historiques restent valides et conservées.
Le premier run exact de mise au point est exclu des statistiques car le script
opérateur attendait le mauvais numéro après une trace native antérieure absente.

NEXT_ACTION : Compiler l’hôte diagnostique en release et remesurer le même run 64 MiB pour réduire et vérifier le coût dominant du chiffrement Noise.

Une seule optimisation ciblée ensuite. Aucun chantier RAM/GPU/NPU/C13, aucune
politique CPU dynamique, aucun changement de protection ou de protocole.
