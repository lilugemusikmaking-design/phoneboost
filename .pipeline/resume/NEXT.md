# Reprise CPU Android : refus ResourceGuard observé

Lire [le rapport](../../docs/architecture/ANDROID_MULTICORE_SPIKE_V0_2.md) et [les preuves](../../docs/evidence/android-multicore-v0-2.json). Le code testé est `93f1de187838163729c476da2c327f5a612cf8a9`, sur master `edad4ffc90c6143d1a4f43dfa6096be21cb97cfa`.

Verdict : **MULTICORE_BLOCKED_BY_RESOURCE_GUARD**. Un contrôle distant64MiB/1thread est exact. Les captures complètes1/2/4 sont refusées avant C10 ; Health montre RefusedMemory/budget0 sur toute la fenêtre des profils2/4. Battery Saver reste actif. Aucun gain ni parallélisme physique démontré. Aucun réglage ni seuil modifié.

NEXT_ACTION : Rejouer la preuve physique Perfetto1/2/4 lorsque Health est naturellement nominale, sans modifier les seuils ResourceGuard; RAM/GPU/NPU restent fermés.

La publication de cette branche dédiée doit passer Actions/CodeQL. La PR reste ouverte tant que le multicœur Android n’a pas été exécuté et qualifié physiquement. Produit et APK `org.phoneboost.app` conservés ; expérimental `org.phoneboost.app.multicore` arrêté après remise à1. Aucune destruction de worktree/branche historique. Ne pas relancer WebP, CAPACITY/THROUGHPUT, audit SoC ou étude below-OS. Ergonomie appairage/verrouillage explicitement différée.
