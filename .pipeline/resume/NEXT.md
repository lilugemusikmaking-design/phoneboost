# Reprise : spike CPU physique fermé, profil 2 retenu

Verdict global : **MULTICORE_CONFIRMED_BENEFICIAL**, limité à BLAKE3 natif.
Preuve 1→2 historique : 1,754×, 6/6 favorables. Profil 4 physique confirmé : 4 TID,
pic à 4 Running, 25,824309 ms à quatre. Gain incrémental 2→4 non qualifié : cinq paires
complètes, 1,065364×, 3/5 favorables ; sixième profil 4 isolé à 69,703308 ms conservé.

Lire le [rapport](../../docs/architecture/ANDROID_MULTICORE_SPIKE_V0_2.md) et les
[preuves 2/4](../../docs/evidence/android-multicore-comparison-2-4-v0-2.json). Le dernier profil 2 n'est jamais soumis : après baisse mémoire
et refus C08, C08 récupère avant 120 s, mais la fenêtre opérateur de sept échantillons
déjà nominaux n'est pas atteinte. Ne pas confondre cet arrêt avec un refus C08
encore actif, ni présenter six paires complètes. Aucune relance à effectuer.

NEXT_ACTION : Diagnostiquer quantitativement les ~28 s end-to-end en décomposant C12 import, transport/PBMUX, copies, readiness/lease et compute, avant toute nouvelle optimisation.

Cette action n'a pas été commencée. RAM/GPU/NPU/C13 et affinité restent fermés.
Produit à 1 thread ; exception 1/2/4 strictement expérimentale ; ResourceGuard,
contrats et plafonds inchangés. PR #32 draft et non fusionnée, aucune fusion
automatique. Vérifier Actions/CodeQL du HEAD publié.

Nettoyage vérifié : Battery Saver ON, profil 1, auto-use OFF, APK expérimental arrêté,
hôte/tunnel privés absents, deux APK inchangés. Anciennes preuves conservées,
leurs NEXT_ACTION historiques remplacés par celui-ci.
