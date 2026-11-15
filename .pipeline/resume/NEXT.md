# Reprise — décomposition PUT mesurée

Branche spike/put-wait-breakdown-v0-1 depuis master a85364d6fec2ae60db09116b7c57a028fac381e1 (#34 merge commit).
PR #35 diagnostique, non fusionnée. Source APK/host mesurés : 25fd92f6a793e8a1224cf885f0600485da5f83a5.
Rapport : docs/architecture/PUT_WAIT_BREAKDOWN_V0_1.md.
Preuves : docs/evidence/put-wait-breakdown-v0-1.json +8 traces numériques JSON.
1 calibration +3 runs exacts/Health nominale ; fenêtres host/Android séparées, pas de double comptage.
Run médian : 3361,505 ms ; SessionPoll3032,870 dont2453,948 pendant17PUT.
Coût fonctionnel directement isolé le plus grand, médiane3runs : decrypt Android737,853ms ; polling réception+retard handler647,456ms.
Transport/USB pur non isolé ; polling inter-PUT chevauche file host/résidu.
SaverON/profil1/collecteOFF ; app/daemon/forward arrêtés, produit intact.
Nouveau wrapper dev Battery Saver automatique/finally : tests passent, Samsung ignore cmd power set-mode0 ; témoin refusé et restaurationON vérifiée. Aucun fallback silencieux.
Ne pas refaire les campagnes closes ; pas de changement protocole/Noise/PBMUX/chunks/ResourceGuard/timeouts/C13/RAM/GPU/NPU.
NEXT_ACTION : Réduire uniquement le coût du déchiffrement Noise Android, puis vérifier le gain end-to-end par A/B sous les mêmes garde-fous, sans changer le protocole.
