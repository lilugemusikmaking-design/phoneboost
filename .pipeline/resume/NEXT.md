# Reprise CPU Android : profil 2 bénéfique sur BLAKE3 natif

État autoritaire courant : **MULTICORE_CONFIRMED_BENEFICIAL**, limité au workload
CPU expérimental BLAKE3 64 MiB sur le Samsung SM-A155F. Lire le
[rapport](../../docs/architecture/ANDROID_MULTICORE_SPIKE_V0_2.md) et les
[preuves de comparaison](../../docs/evidence/android-multicore-comparison-1-2-v0-2.json).
L'APK testé reste issu de93f1de187838163729c476da2c327f5a612cf8a9 ; aucun code produit
n'a changé. Ce checkpoint ajoute les preuves et réduit uniquement le buffer
Perfetto64→16MiB, avec durée55s/événements/SQL inchangés.

Deux warmups avec captures scheduler complètes, puis six paires équilibrées,
14résultats distants exacts. Le profil2 montre2TID surCPU6/7 et50,782463ms de vrai
Running simultané. Accélération native médiane appariée1,753576×,6/6paires>1,05×.
Aucun gain end-to-end ni avantage produit qualifié. Les refus et premières traces
restent historiques, leurs anciens NEXT_ACTION sont remplacés par celui-ci.

NEXT_ACTION : tester le profil 4 sous les mêmes garde-fous.

Avant tout prochain job : APK expérimental exact, ADB/USB stable, HAL Ready sans
override, Battery Saver OFF pour la campagne puis restauré, Health fraîche et
nominale, récupération naturelle896MiB/10s et budget suffisant. Ne modifier ni
ResourceGuard, C03/C08/C10/C13, plafonds mémoire, affinité ni politique de threads.
RAM/GPU/NPU restent fermés. Profil4 non exécuté dans cette campagne ; PR32 draft
et non fusionnée. Vérifier les Actions/CodeQL du HEAD publié, sans inférer leur
succès des tests locaux.

Nettoyage terminé : Battery SaverON vérifié dans Android et Health, profil1,
auto-useOFF, APK de test arrêté, daemon/tunnel privés absents. APK produit et
expérimental inchangés. Aucun worktree historique supprimé ni modifié.
