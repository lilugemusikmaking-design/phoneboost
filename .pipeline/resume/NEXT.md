# Reprise : diagnostic quantitatif du temps end-to-end C12

Base : `master` après fusion de #32, `524d0065f5609919c61e37c242771ee3bf61eb9b`.
Branche : `spike/c12-end-to-end-timing-v0-1`.
La PR #32 est fusionnée. Le verdict CPU expérimental antérieur reste dans
[son rapport](../../docs/architecture/ANDROID_MULTICORE_SPIKE_V0_2.md).

Le [plan de mesure](../../docs/architecture/C12_END_TO_END_TIMING_DIAGNOSTIC_V0_1.md)
repère les bornes CLI, import asynchrone, copies, PBMUX, RemoteBuffer et compute,
ainsi que l'état de readiness/lease. Le nouveau chantier est **préparé seulement** :
aucune instrumentation codée ni nouvelle mesure physique n'a encore eu lieu.
Ne pas déduire le goulot des ~28 s du seul temps natif déjà mesuré.

NEXT_ACTION : Diagnostiquer quantitativement les ~28 s end-to-end en décomposant C12 import, transport/PBMUX, copies, readiness/lease et compute, avant toute nouvelle optimisation.

Commencer par instrumenter ces bornes dans le seul diagnostic, valider les
mesures sans changement de réponse ni de protocole, puis faire les runs
physiques exacts. Garder les horloges hôte et Android séparées. Aucun chantier
RAM/GPU/NPU/C13, aucune optimisation ni décision de politique CPU à ce stade.
