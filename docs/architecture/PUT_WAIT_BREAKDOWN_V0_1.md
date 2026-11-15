# Décomposition locale des PUT — diagnostic v0.1

Base : master `a85364d6fec2ae60db09116b7c57a028fac381e1`, merge commit de #34
(HEAD `8c7a45722f70b5180ee33fc117e4d4eddab686cf`). Les deux checks étaient verts,
aucune review ou thread bloquant ; branches et worktrees historiques conservés.
Branche : `spike/put-wait-breakdown-v0-1`.

## Portée et statut

Instrumentation uniquement ; aucune optimisation. Les anciens résultats de #34
sont historiques, pas une baseline directe contre un nouvel APK. L'utilisateur
a autorisé le 15 novembre l'installation d'un nouvel APK `org.phoneboost.app.multicore`
contenant uniquement les sondes nécessaires. `org.phoneboost.app` reste intact.
Un seul SHA APK sera utilisé pendant toute la campagne.

Le protocole, PBMUX/Noise, les tailles des PUT, RemoteBuffer, ResourceGuard,
les timeouts, le polling Android, C13 et les profils CPU ne sont pas modifiés.
Pas de RAM distante/GPU/NPU/affinité. Le profil expérimental 2 est seulement
utilisé pour reproduire le parcours existant.

## Collecte

Feature existante `diagnostic-timing`, collecte agrégée existante
`PHONEBOOST_TIMING_DIAGNOSTIC=1`, et opt-in supplémentaire
`PHONEBOOST_PUT_DIAGNOSTIC=1` pour la trace host. Sur l'APK expérimental, le
receiver protégé par DUMP accepte `enabled=1, put_trace=1`. Aucun champ réseau.
La collecte est absente des builds produit sans la feature.

Les lignes numériques bornées (32 768, au plus 1,5 MiB de stockage primaire)
contiennent : type, pseudonyme de requête, thread local, début, fin et valeur
numérique (taille, flags existants ou jeton caller local). Le pseudonyme est
un SHA-256 tronqué à 48 bits du request_id existant. Pas de payload, session ID,
handle, clé, chemin privé ou digest de données dans ces lignes. Une collision,
une perte ou un dépassement rend l'analyse non qualifiante.

Le host exporte après fermeture du client local, hors opération mesurée ; Android
exporte après le run dans `files/put-timing.json`, via le seul receiver diagnostique.
Les compteurs agrégés existants ne changent pas de contrat.

Le point « record disponible » est précisément le premier `peek` observant les
deux octets du préfixe. Ce n'est ni un timestamp matériel USB ni la preuve que
tout le record est arrivé. `WireReadBody` peut contenir une attente réseau.
La copie RemoteBuffer est imbriquée dans l'exécution du handler. Le polling
attendant le handler chevauche cette exécution : le retard **après fin du
handler jusqu'à constatation de sa réponse** est aussi mesuré séparément.

## Attribution sans mélange d'horloges

`scripts/analyze_put_wait.py` joint les 17 PUT par pseudonyme et vérifie
fragments, tailles, succès caller, ordre local et complétude. Les durées du
host et d'Android restent dans leurs domaines respectifs. Les tableaux sont
des vues superposées, jamais une addition des durées imbriquées.

La partition host complète est ready → enqueue → thread session → fin send →
détection réponse → notification → réveil caller. Les détails crypto/write
sont imbriqués dans send. La somme des `SessionPoll` pendant les PUT est
séparée des autres RPC du job ; les ~3,07 s antérieures n'étaient pas une mesure
exclusive des PUT.

Un résidu de durées utilise seulement une relation causale : le traitement
Android entre première réception et début d'envoi réponse est contenu dans
l'intervalle host « début traitement requête → détection réponse ». Leur différence
contient notamment début d'émission host, envoi retour, transport et ordonnancement.
Ce n'est pas un RTT USB. Le chiffrement host chevauche Android et ne doit pas
en être soustrait une seconde fois. Les timestamps des deux machines ne sont
jamais soustraits ni alignés.

## Validation et mesure

Tests release host/CLI/runtime, vérification C12 OFF/ON et comparaison des sorties
avec le host #34 ; analyseur testé contre décalage arbitraire d'horloge Android,
fragments manquants, réponse en échec, perte et doublon. Build/lint Android et tests runtime avec sondes activées réussis au checkpoint logiciel initial.

Campagne physique à venir : une calibration puis trois runs, 64 MiB identiques,
profil 2, release standard, USB/ADB, digest exact et REMOTE_SUCCESS obligatoires.
Préflight existant : HAL Ready sans override, Battery Saver OFF, Health fraîche
et nominale, récupération mémoire >=896 MiB pendant >=10 s, budget suffisant.
Aucun abaissement de seuil ni relance aveugle. Restaurer Saver ON, profil 1,
collecte OFF et arrêter les processus propres à la campagne.

Aucune conclusion causale nouvelle ni NEXT_ACTION d'optimisation avant la mesure.

NEXT_ACTION : terminer la validation, puis calibrer et mesurer les PUT sous les garde-fous existants.
