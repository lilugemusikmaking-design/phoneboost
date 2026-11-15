# Décomposition physique des PUT — v0.1

Base master après merge commit de #34 : `a85364d6fec2ae60db09116b7c57a028fac381e1`.
Branche : `spike/put-wait-breakdown-v0-1`. PR #35, aucune fusion automatique.

## Résultat

Une calibration puis **trois runs complets valides**, même nouvel APK diagnostique,
mêmes binaires release, input 64 MiB, digest exact et REMOTE_SUCCESS. Les anciennes
mesures #34 sont historiques : aucun gain comparatif n'est revendiqué contre elles.

Le run médian par CliTotal est **run2** : **3361.505 ms** (process CLI 3363.785 ms).
Le `SessionPoll` global vaut **3032.870 ms** : **2453.948 ms pendant les 17 PUT**,
et **578.921 ms hors de ces PUT** (autres RPC/intervalles du job).
Les ~3,07 s ne représentaient donc pas uniquement les PUT.

## Build et isolation

Source des binaires/APK : `25fd92f6a793e8a1224cf885f0600485da5f83a5`.

- APK `org.phoneboost.app.multicore` SHA-256 : `efc70d8adb6e00302ae6ca8edfd5f6fee69f4466d30cad08f6c3b59c09791a6e`.
- Bibliothèque JNI embarquée SHA-256 : `d91727d39f36505fea42f21594565cce49515edaafdc59dafc0e2f70512cb69b`.
- phoneboostd SHA-256 : `28ca97e74b5503970078e913c39d2f82d020fb4a45d538ac96aaa67b673af6ec`.
- phoneboostctl SHA-256 : `668f3d1f4b071542eaffc030893602646a10dc20ce0600af57ce113c696794c6`.

Cargo release standard, rustc 1.98.0, un job build, aucun RUSTFLAGS/wrapper/override
CARGO_PROFILE. Features host : experimental-multicore + diagnostic-timing ; CLI :
diagnostic-timing ; JNI : diagnostic-timing (inclut experimental-multicore).
Dépendances, Cargo.lock, protocole, PBMUX/Noise, PUT, timeouts, polling Android,
RemoteBuffer, ResourceGuard, seuils, C13 et licence inchangés. Pas de RAM/GPU/NPU.
Le produit `org.phoneboost.app` est resté au hash initial.

Sondes compilées uniquement avec diagnostic-timing et activées en plus par
PHONEBOOST_PUT_DIAGNOSTIC=1 / extra receiver put_trace=1. Stockage numérique borné
à 32 768 lignes (~1,5 MiB) ; compteur de perte obligatoire zéro. Aucun handle,
session ID brut, clé ou contenu ; tags SHA-256 tronqués du request_id existant.
L'export host et l'export Android sont post-run. Le coût total des sondes n'a pas
été isolé physiquement : il reste inclus dans les intervalles et résidus. Les
temps de fonctions sont des durées murales locales, pas du CPU time exclusif.

## Préflight et restauration

Samsung SM-A155F, Android 16/API 36,
`samsung/a15nseea/a15:16/BP4A.251205.006/A155FXXSBEZG1:user/release-keys`.
USB/ADB, profil 2, Battery Saver OFF pendant les quatre runs. HAL Ready sans
override, Health fraîche/nominale, récupération >=896 MiB pendant >=10 s,
budget 128 MiB, aucune admission refusée. Les preuves de chaque préflight sont
conservées dans le JSON. La mémoire disponible des trois répétitions était
entre 1101,9 et 1222,2 MiB aux bornes ; âge natif maximal 1620 ms ; thermique 0.

Fin vérifiée : Saver ON, profil 1, collecte OFF, auto-use OFF, daemon/forward propres
et APK expérimental arrêtés, produit intact. Aucun stress mémoire/thermique.

## Run médian : vues locales, non additives

Dénominateur de la colonne pourcentage : **2773.928 ms de durée caller cumulée des 17 PUT**.
Ces lignes se chevauchent ; la partition additive host est donnée juste après.

| Composante | durée_ms | % du temps PUT | Preuve / portée |
|---|---:|---:|---|
| File host avant send | 357.417 | 12.88 % | enqueue → traitement session |
| Crypto host | 86.194 | 3.11 % | Encrypt dans les frames PUT ; incluse dans send |
| Socket write host | 30.242 | 1.09 % | WireWrite ; incluse dans send |
| Attente host après send | 2143.445 | 77.27 % | fin send → premier préfixe réponse observé |
| Traitement/réception réponse host | 89.002 | 3.21 % | préfixe observé → notification |
| Dont lecture réponse host | 88.153 | 3.18 % | imbriquée ; principalement read_exact bloquant |
| File après réponse host | 0.472 | 0.02 % | notification → réveil caller |
| Lectures record Android | 522.744 | 18.84 % | read_exact prefix/body ; attente et copie noyau non séparées |
| Decrypt Android | 602.170 | 21.71 % | snow read_message, PUT seulement |
| Réassemblage Android | 26.251 | 0.95 % | Reassembler.accept |
| Queue handler Android | 11.159 | 0.40 % | enqueue → entrée handler |
| Exécution handler Android | 27.350 | 0.99 % | entrée → fin, contient la copie |
| Copie RemoteBuffer | 26.593 | 0.96 % | imbriquée dans handler |
| Réponse Android | 8.357 | 0.30 % | constat résultat → réponse construite/chiffrée/écrite |
| Polling Android entre records PUT | 345.182 | 12.44 % | sleeps réellement observés pendant réception PUT |
| Retard après fin du handler | 313.330 | 11.30 % | fin réelle → constatation par thread session |
| Polling handler brut (overlay) | 345.708 | 12.46 % | chevauche queue/exécution ; ne pas additionner |
| Polling Android inter-PUT (overlay) | 731.809 | 26.38 % | 16 intervalles ; chevauche file host et résidu |
| Hors noyau temporel Android | 347.614 | 12.53 % | résidu causal : début send host + envoi retour/transport/ordonnancement |
| Autre local Android | 89.870 | 3.24 % | allocations, dispatch, instrumentation et ordonnancement non séparés |

Partition host additive : avant enqueue 0.077 + file
357.417 + send complet 183.514 + attente
après send 2143.445 + réception/traitement réponse
89.002 + réveil 0.472
= 2773.928 ms. Send contient crypto/write/encode et
52.469 ms de construction/copies/dispatch/ordonnancement non séparés.
Le polling host mesuré pendant la file est 312.103 ms,
et pendant l'attente réponse 2141.555 ms.

Autre partition causale : file/caller avant send + intervalle local Android
« premier record observé → début envoi réponse » (1979.346 ms)
+ résidu hors de cet intervalle (347.614 ms)
+ réception/notification/réveil host = durée caller. Seules des **durées** locales
sont soustraites ; aucun alignement ni soustraction d'epochs host/Android.
Le résidu n'est pas du transport pur et chevauche du travail host d'émission.

« Record disponible » signifie préfixe de deux octets vu par peek, pas record
complet ni timestamp matériel. Une lecture body bloquante peut contenir attente
ADB/USB/TCP, ordonnancement et copies noyau. L'effet de chacun reste non séparé.

## Les 17 PUT du run médian

16 blocs de 4 194 240 octets + un tail de 1024 octets ; 1105 frames requête PUT,
17 frames réponse PUT. Le job entier conserve 1113/25 frames applicatives hors
Control/Metrics. La readiness de démarrage contient aussi un PUT de 1 octet,
antérieur au job et comptabilisé séparément ; elle n'est pas une 18e entrée 64 MiB.

| PUT | Caller ms | File host ms | Wait après send ms | Decrypt Android ms | Poll réception ms | Retard fin handler ms | Résidu hors noyau Android ms |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 174.036 | 18.205 | 140.394 | 43.679 | 20.203 | 18.685 | 22.478 |
| 2 | 164.888 | 17.935 | 136.343 | 49.858 | 20.142 | 18.706 | 20.637 |
| 3 | 169.354 | 18.436 | 142.134 | 35.615 | 20.686 | 18.252 | 21.424 |
| 4 | 162.885 | 18.611 | 134.951 | 37.316 | 20.167 | 18.389 | 20.326 |
| 5 | 156.163 | 17.089 | 129.482 | 41.231 | 20.282 | 18.215 | 20.809 |
| 6 | 212.439 | 18.721 | 131.233 | 40.270 | 40.739 | 18.235 | 20.469 |
| 7 | 164.791 | 18.699 | 133.320 | 30.735 | 40.376 | 18.594 | 14.602 |
| 8 | 157.858 | 18.752 | 128.117 | 26.004 | 20.407 | 18.808 | 21.679 |
| 9 | 152.484 | 18.354 | 125.179 | 24.037 | 20.354 | 18.796 | 20.873 |
| 10 | 154.833 | 18.759 | 123.845 | 23.382 | 20.416 | 18.256 | 21.100 |
| 11 | 159.048 | 18.668 | 123.482 | 28.033 | 20.407 | 17.366 | 21.692 |
| 12 | 163.288 | 18.536 | 132.292 | 39.044 | 20.157 | 18.057 | 21.248 |
| 13 | 158.785 | 17.055 | 131.262 | 45.827 | 20.302 | 17.726 | 19.778 |
| 14 | 204.497 | 62.260 | 130.756 | 50.624 | 0.000 | 18.511 | 18.176 |
| 15 | 158.619 | 18.595 | 130.315 | 44.933 | 20.415 | 18.419 | 20.842 |
| 16 | 157.423 | 18.685 | 128.190 | 41.558 | 20.129 | 18.310 | 20.321 |
| 17 | 102.537 | 20.059 | 42.152 | 0.024 | 0.000 | 20.006 | 21.161 |

Distribution caller des 17 PUT : médiane **159.048 ms**, min **102.537**, max **212.439**.
Bins `<100 / 100–150 / 150–200 / >=200 ms` : **[0, 1, 14, 2]**. Le tail est de taille différente.
Deux outliers de lecture de réponse dans run2 : 47,919 ms sur un bloc plein
(PUT 6) et 40,107 ms sur le tail ; cette attente est incluse dans réception host,
pas dans le decrypt. Le PUT 14 atteint 204,497 ms, dont 62,260 ms en file host
(seulement 17,127 ms de SessionPoll dans cette file ; le reste reste ordonnancement/traitement de session non isolé). L'hypothèse TCP/ADB/scheduling reste une hypothèse, pas une
preuve de delayed ACK/Nagle ni une mesure de capacité USB.

## Stabilité et classement

| Run | CliTotal ms | PUT caller ms | Decrypt Android ms | Poll réception + retard handler ms | Read record Android ms | File host ms |
|---|---:|---:|---:|---:|---:|---:|
| run1 | 3360.503 | 2722.513 | 737.853 | 619.721 | 524.239 | 299.215 |
| run2 | 3361.505 | 2773.928 | 602.170 | 658.512 | 522.744 | 357.417 |
| run3 | 3384.460 | 2763.279 | 748.519 | 647.456 | 457.493 | 311.225 |

Classement des coûts **directement isolés**, par médiane des trois runs (distincte
du run médian présenté ci-dessus) :

1. Decrypt Android : **737,853 ms** dans les appels snow read_message.
2. Polling Android dans la réception + attente après fin réelle du handler :
   **647,456 ms** ; aucune addition du poll handler brut avec l'exécution.
3. Lectures Android : **522,744 ms**, mélange bloquant non attribué entre transport,
   copies noyau et ordonnancement ; **pas un débit USB**.
4. Résidu hors noyau temporel Android : **354,265 ms**, dont début émission host et
   acheminement/ordonnancement ; chevauchements explicitement conservés.
5. File host avant send : **311,225 ms**. Réveil caller <1 ms médian.
6. Handler : **26,480 ms**, dont copie **25,790 ms**. Il ne domine pas.

Le polling Android inter-PUT (16 gaps) est ~650–732 ms supplémentaires observés,
mais chevauche la file host et le résidu : **il ne s'ajoute pas à ce classement**.
La plus grosse fonction individuellement isolée est le decrypt ; l'attribution
exclusive de toutes les attentes entre USB/ADB/scheduler/polling reste partielle.
Le run2 a un decrypt plus court compensé par des attentes ailleurs : aucune
promesse de gain end-to-end à partir de la seule diminution du decrypt.

## Validation, incidents et outillage Battery Saver

Tests Rust release, tests runtime sondes ON, C12 OFF/ON et sorties #34/new équivalentes,
compilation sans feature, build/lint Android, analyseur déterministe et validation
des quatre traces réelles ; Gitleaks avant chaque checkpoint. Les tests et preuves
rejouables sont dans scripts/test_analyze_put_wait.py et
scripts/validate_put_wait_evidence.py. Les huit traces numériques JSON sont publiées,
avec hashes, ainsi que les mesures structurées. Les logs bruts privés ne le sont pas.

Trois ajustements du banc sont conservés : attente READY initiale expirée avant
job puis récupération naturelle observée ; exclusion explicite du PUT readiness
antérieur ; récolte du daemon enfant terminé entre répétitions. Aucune modification
fonctionnelle ni de timeout PhoneBoost, aucun run valide éliminé.

À la demande complémentaire de l'utilisateur, wrapper automatique/réversible
Battery Saver et branchement du launcher Perfetto :
[mode d'emploi et résultat physique](../testing/PHYSICAL_CAMPAIGN_POWER.md).
Sur ce Samsung, cmd power set-mode 0 est sans effet : témoin refusé, sortie 2,
finally vérifié ON. Le chemin automatique réussi n'est pas physiquement qualifié ;
le script ne remplace pas cette absence de confirmation par une valeur nominale.
Aucune nouvelle campagne de performance ni modification produit.

## NEXT_ACTION

Réduire uniquement le coût du déchiffrement Noise Android, puis vérifier le gain end-to-end par A/B sous les mêmes garde-fous, sans changer le protocole.
