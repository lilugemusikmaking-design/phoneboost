# Reprise du spike CPU Android v0.2

Base master `edad4ffc90c6143d1a4f43dfa6096be21cb97cfa`, #31 fusionnée. Branche `spike/android-multicore-v0-2`. Lire [le rapport](../../docs/architecture/ANDROID_MULTICORE_SPIKE_V0_2.md) et [les preuves](../../docs/evidence/android-multicore-v0-2.json).

NEXT_ACTION : exécuter la campagne physique préparée 1/2/4 et conserver les refus ResourceGuard. Le logiciel est validé ; aucune preuve physique CPU à ce checkpoint.

Exception expérimentale utilisateur : profils explicites 1/2/4 seulement. Produit à 1 ; aucun C13 dynamique, aucune affinité, RAM/GPU/NPU hors scope. Worktrees historiques conservés ; collecteur quarantiné exclu. Ne pas relancer les anciennes campagnes WebP/CAPACITY/THROUGHPUT ni les audits SoC/below-OS.
