# Reprise CPU Android : multicœur réel confirmé, performance à qualifier

Verdict intermédiaire : **MULTICORE_EXECUTION_CONFIRMED**. Lire le
[rapport](../../docs/architecture/ANDROID_MULTICORE_SPIKE_V0_2.md) et les
[preuves scheduler](../../docs/evidence/android-multicore-scheduler-1-2-v0-2.json).
Le code APK testé reste93f1de187838163729c476da2c327f5a612cf8a9 ; cette publication
ne change que les preuves et la durée Perfetto40→55s explicitement autorisée.

Une capture complète exacte64MiB par profil1/2. Le profil2 montre deux TID sur
CPU5/6/7, maximum2Running simultanément, overlap49,913692ms. Health nominale,
HAL Ready sans override, Battery Saver désactivé manuellement pour les essais.
Aucun seuil, C13, affinité ou comportement produit modifié. Le profil4 n'est pas lancé.

NEXT_ACTION : effectuer la comparaison physique reproductible 1 vs 2 ; profil 4 seulement ensuite.

Ne pas fusionner #32 : aucun verdict de performance reproductible/avantage produit
n'est établi. Les anciens refus et la trace40s incomplète sont conservés, mais ne
sont ni une baseline qualifiée ni une preuve multicœur. Les traces brutes privées
et leurs hashes sont référencés par les preuves. Ne pas relancer la campagne1/2/4
en bloc, ni ouvrir RAM/GPU/NPU/C13. Produit et worktrees historiques intacts.

Nettoyage terminé : Battery Saver réactivé manuellement par l’utilisateur et
vérifié dans Health et le service Android ; profil expérimental 1, APK de test
arrêté, auto-use OFF, daemon et tunnel privés absents. Les deux APK sont inchangés.
Aucun nouveau run effectué. L’échec antérieur de la commande native reste
historique ; il n’est pas la preuve de cette restauration manuelle.
