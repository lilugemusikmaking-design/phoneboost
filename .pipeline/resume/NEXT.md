# Reprise — attente session host mesurée

Branche spike/session-input-wait-v0-1 depuis master 21f664783d333024d9b336a7423dfb7e197a9318.
#33 est fusionnée avec merge commit ; branches/worktrees historiques préservés.

Rapport : docs/architecture/SESSION_INPUT_WAIT_V0_1.md.
Preuves : docs/evidence/session-input-wait-v0-1.json.
Host-only rustix poll borné20ms, partial prefix sans spin/FIN fail-closed ; APK inchangé.
Deux warmups et trois paires exactes : 3.416→3.294s médian, ratio médian apparié1.031819x.
Gain modeste variable ; petites requêtes pas améliorées. CPU idle host0.4% dans les deux groupes.
Restant : host SessionPoll3.073s ; répartition filehost/Android non isolée.
Ne pas refaire les campagnes closes ni optimiser crypto/transport/chunks/RAM/GPU/NPU/C13.

BatterySaverON, profil1, collecteOFF, app/daemon/forward arrêtés ; produit intact.
PR polling à vérifier sur GitHub, pas de merge automatique.

NEXT_ACTION : Décomposer les 3,07 s d’attente résiduelle des PUT entre file host et polling Android avant toute seconde optimisation.
