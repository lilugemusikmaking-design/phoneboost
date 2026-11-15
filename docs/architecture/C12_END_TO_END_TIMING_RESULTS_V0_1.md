# C12 — mesure physique du temps end-to-end v0.1

Diagnostic terminé le 2026-11-15, sans optimisation. Base `master`
`524d0065f5609919c61e37c242771ee3bf61eb9b`, plan validé au checkpoint
`81df7e20dc010fe1d8256cfebccf847dc27a639f`.
Le [plan existant](C12_END_TO_END_TIMING_DIAGNOSTIC_V0_1.md) est exécuté sans
redéfinition de son périmètre. [Données numériques](../evidence/c12-end-to-end-timing-v0-1.json).

## Résultat démontré

Trois runs complets de 64 MiB donnent `REMOTE_SUCCESS` et le digest exact.
Le chiffrement `snow::TransportState::write_message` côté hôte occupe
**91,72–92,42 %** du temps mesuré (23,951–25,958 s). L'hôte est le même type
de build **dev non optimisé avec debuginfo** que les campagnes précédentes ;
le JNI Android est compilé en release. Le diagnostic ne change pas ces profils.
Le CPU time du processus hôte est 24,00 / 24,42 / 25,88 s, cohérent avec ce poste.
Le compute Android ne dure que 49,785–51,486 ms (médiane 50,734 ms).

Le deuxième poste distinct mesuré sur la boucle hôte est l'attente/poll de
session : 1,688–1,750 s (environ 6–7 %). Sa part n'est pas un RTT USB pur :
elle inclut l'attente de disponibilité des réponses et la cadence de polling.
Android mesure parallèlement 1,436–1,680 s de déchiffrement et environ 407 ms
de polling des handlers RemoteBuffer, qui chevauchent le chemin hôte.
Ces durées Android ne s'ajoutent pas à la partition Linux et ne démontrent pas
à elles seules la causalité de chaque milliseconde d'attente hôte.

| Run complet | end-to-end ms | chiffrement hôte ms | part | natif ms | débit utile MiB/s |
| --- | ---: | ---: | ---: | ---: | ---: |
| run02 | 26112.151 | 23950.994 | 91.72 % | 50.734 | 2.451 |
| run03 | 26502.267 | 24413.523 | 92.12 % | 51.486 | 2.415 |
| run04 | 28085.955 | 25958.191 | 92.42 % | 49.785 | 2.279 |

La variation du temps total ne déplace pas le goulot. Le premier run de mise
au point a aussi produit un résultat distant exact, mais son collecteur s'est
arrêté après le calcul : l'absence de trace native antérieure est codée `-1`,
pas `0`. Cette assertion du script opérateur a été corrigée ; aucune fonction
PhoneBoost n'a changé. Ses données brutes sont conservées et **exclues** des
statistiques, qui portent uniquement sur les trois relevés complets suivants.

## Partition du run médian en end-to-end

Les valeurs suivantes proviennent toutes de **run03**, celui dont le temps
total est médian, et non d'une somme incohérente de médianes indépendantes.
Total CLI processus : **26502.267 ms**. Débit utile : **2.414888 MiB/s**
(2,532 MB/s décimaux). Le total inclut création/fin du processus CLI ; le
préflight, les commandes ADB et la collecte postérieure sont exclus.

| phase | durée_ms | % total | octets | débit_effectif | appels/frames | attente_ms | remarque |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| CLI / connexion locale | 0.040067 | 0.0002 % | — | — | 1 | — |  |
| imports.create | 0.442853 | 0.0017 % | — | — | 1 | — |  |
| attente import READY | 51.695231 | 0.1951 % | — | — | 3 polls / 2 sleeps | 50.193 | inclut la copie C12 asynchrone |
| consommation du handle C12 | 0.056076 | 0.0002 % | — | — | 1 | — |  |
| attente verrou auto-use | 0.002057 | 0.0000 % | — | — | 1 | — |  |
| placement auto-use | 0.006050 | 0.0000 % | — | — | 1 | — |  |
| RemoteBuffer réservation + commit | 62.542389 | 0.2360 % | 67108864 | — | 1 | — |  |
| ALLOC (aller-retour) | 61.011979 | 0.2302 % | 67108864 | — | 1 | — |  |
| boucle PUT entière | 25967.340949 | 97.9816 % | 67108864 | 2.465 MiB/s | 17 PUT | 25934.351 | RTT logiciel incluant chiffrement local, transport, worker et ACK |
| STAT (aller-retour) | 61.213107 | 0.2310 % | — | — | 1 | — |  |
| scratch réservation + commit | 124.175668 | 0.4685 % | 8388608 | — | 1 | — |  |
| compute.submit (aller-retour) | 82.042587 | 0.3096 % | 67108864 | — | 1 | — | inclut 51.486 ms de compute natif |
| compute.status | 0.000000 | 0.0000 % | — | — | 0 | — |  |
| poll compute / sleep | 0.000000 | 0.0000 % | — | — | 0 | — |  |
| libération distante | 81.007144 | 0.3057 % | — | — | 1 | — |  |
| validation retour résultat | 0.011468 | 0.0000 % | — | — | 1 | — |  |
| résidu local / processus | 10.679620 | 0.0403 % | — | — | — | — | début/fin CLI, traitement local, sérialisation/réponse/close et petits gaps non isolés |

Cette partition exclusive somme à 100 %. Le résidu est **10.679620 ms**
(**0.0403 %**). Il reste non attribué à une opération précise :
validation locale, enveloppe JSON/réponse, fermeture du fichier, début/fin du
processus et petits intervalles entre sondes. Il n'est pas imputé au réseau.
Le seul résidu interne à `HostTotal` est 0,032222 ms.

## Décomposition de la boucle session hôte

Cette vue est une **autre partition** de la fenêtre C12, imbriquée dans la
précédente, pas une série de lignes à lui additionner. Les spans du thread de
session sont disjoints entre eux. Le compteur session est pris à l'ouverture
et à la fermeture de la connexion C12 contrôlée. Le petit trafic de contrôle
concurrent reste explicitement compté ; aucun intervalle hors job n'est imputé
à un acquire de bail synchrone. Le résidu de cette vue inclut notamment le temps
passé hors boucle pendant l'import, et ne remplace pas le résidu précédent.

| phase | durée_ms | % total | octets | débit_effectif | appels/frames | attente_ms | remarque |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| construction / fragmentation requêtes | 41.862809 | 0.1580 % | — | — | 27 | — | fenêtre C12, contrôle inclus |
| encodage frames PBMUX | 38.794066 | 0.1464 % | 67110480 | — | 1124 | — | fenêtre C12, contrôle inclus |
| chiffrement Noise HÔTE | 24413.523321 | 92.1186 % | 67155440 | 2.621 MiB/s | 1124 | — | fenêtre C12, contrôle inclus |
| write record chiffré | 137.098254 | 0.5173 % | 67175672 | — | 1124 | — | fenêtre C12, contrôle inclus |
| attente/poll boucle session | 1730.263937 | 6.5287 % | — | — | 86 | 1730.264 | fenêtre C12, contrôle inclus |
| read préfixe | 0.150518 | 0.0006 % | 100 | — | 50 | — | fenêtre C12, contrôle inclus |
| read corps réponse | 71.326468 | 0.2691 % | 6912 | — | 50 | — | fenêtre C12, contrôle inclus |
| déchiffrement réponses | 14.405201 | 0.0544 % | 6912 | — | 50 | — | fenêtre C12, contrôle inclus |
| décodage frames | 0.424356 | 0.0016 % | 6112 | — | 50 | — | fenêtre C12, contrôle inclus |
| réassemblage réponses | 0.021469 | 0.0001 % | 2000 | — | 20 | — | fenêtre C12, contrôle inclus |
| résidu fenêtre session hôte | 52.734266 | 0.1990 % | — | — | — | — | inclut les portions hors boucle, dont import ; différent du résidu du chemin critique |

Le chiffrement porte sur 67 155 440 octets PBMUX dans run03, dont 532 octets
de frames Control/Metrics ; la mesure n'est donc pas présentée comme une
mesure cryptographique de seuls octets utiles. `write` mesure l'appel socket
et son blocage, pas le temps physique intégral du câble. `ClientResponseWait`
est une durée logicielle englobant file, chiffrement, transfert, worker et ACK.
Sa somme (27,901 s) comprend un renouvellement concurrent et **ne forme pas**
une partition du temps total. Aucune mesure de RTT réseau pur n'est revendiquée.

## Import, PUT, copies et Android

| phase | durée_ms | % total | octets | débit_effectif | appels/frames | attente_ms | remarque |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| C12 read | 14.552100 | 0.0549 % | 67108864 | — | 1025 | — | sous-étape déjà incluse dans la partition ; 1024 lectures utiles + EOF, octets inférés des writes exacts |
| C12 write | 23.987450 | 0.0905 % | 67108864 | — | 1024 | — | sous-étape déjà incluse dans la partition |
| C12 flush | 0.000412 | 0.0000 % | — | — | 1 | — | sous-étape déjà incluse dans la partition |
| C12 fsync | 0.004272 | 0.0000 % | — | — | 1 | — | sous-étape déjà incluse dans la partition |
| C12 rename / finalisation / verrou | 0.036912 | 0.0001 % | — | — | 1 | — | sous-étape déjà incluse dans la partition |
| lecture / allocation / préparation PUT | 32.821398 | 0.1238 % | 67108864 | — | 17 | — | sous-étape déjà incluse dans la partition |
| PUT blocs pleins | 25872.231002 | 97.6227 % | 67107840 | — | 16 | — | sous-étape déjà incluse dans la partition |
| PUT bloc final | 62.120092 | 0.2344 % | 1024 | — | 1 | — | sous-étape déjà incluse dans la partition |
| Android allocation buffer | 6.908231 | 0.0261 % | 67108864 | — | 1 | — | durée Android locale, chevauche les RTT hôte |
| Android copie PUT vers buffer | 25.322460 | 0.0955 % | 67108864 | — | 17 | — | durée Android locale, chevauche les RTT hôte |
| Android réassemblage | 48.188791 | 0.1818 % | 67110112 | — | 1108 | — | durée Android locale, chevauche les RTT hôte |
| Android parse / copie requête | 42.254079 | 0.1594 % | 67110112 | — | 20 | — | durée Android locale, chevauche les RTT hôte |
| Android déchiffrement | 1679.513917 | 6.3372 % | 67173424 | — | 1124 | — | durée Android locale, chevauche les RTT hôte |
| Android polling handlers buffer | 407.322305 | 1.5369 % | — | — | 20 | 407.322 | durée Android locale, chevauche les RTT hôte |
| Android polling handler compute | 61.394077 | 0.2317 % | — | — | 3 | 61.394 | durée Android locale, chevauche les RTT hôte |
| Android attente verrou JNI | 0.038847 | 0.0001 % | — | — | 41 | — | durée Android locale, chevauche les RTT hôte |
| Android compute natif | 51.486154 | 0.1943 % | 67108864 | 1243.053 MiB/s | 1 | — | BOOTTIME Android ; CPU time 100.060 ms |

- **Import C12** : attente READY 51,695 ms, dont copie asynchrone read/write
  38,539 ms ; 3 polls et 2 sleeps CLI totalisant 50,193 ms. Cette attente
  chevauche la copie, elle ne s'y ajoute pas. Le premier run de calibration
  avait davantage de lecture à froid ; il est conservé séparément.
- **PUT** : 17 requêtes, exactement **16 × 4 194 240 octets + 1 × 1 024 octets**,
  soit 67 108 864 octets utiles. Préparation locale 32,821 ms. Appels PUT cumulés
  25 934,351 ms, durée RTT **logicielle**, dominée par le chiffrement local ;
  boucle entière 25 967,341 ms, débit effectif 2,465 MiB/s.
- **Copies** : la copie directe Android vers RemoteBuffer est isolée (25,322 ms,
  0,0955 %). La préparation hôte (32,821 ms), le réassemblage Android (48,189 ms),
  le parsing/copie Android (42,254 ms), la construction hôte (41,863 ms) et
  l'encodage (38,794 ms) incluent aussi allocation, contrôles ou lecture. Aucun
  « temps total de memcpy » fictif n'est obtenu en additionnant ces scopes.
- **Disque** : l'entrée et le cache C12 sont sur `tmpfs`, comme dans le banc
  précédent. `fsync` = 0,004 ms ; aucun coût SSD réel n'est qualifié par ce run.
- **Frames** : hôte TX = 1 123 / 1 124 / 1 123 ; RX = 48 / 50 / 49. Hors
  Control/Metrics, **1 113 TX / 25 RX** à chaque run ; les 17 PUT sont fragmentés
  en **1 105 frames de données PUT**. Les 3 autres frames RemoteBuffer sont
  ALLOC/STAT/FREE. Les compteurs TX hôte/RX Android et les octets chiffrés
  concordent sur chaque run complet. La variation Control/Metrics est conservée.
- **Attente** : 84–87 sleeps session hôte, 20 sleeps handler buffer Android et
  3 sleeps handler compute ; **0 compute.status / 0 poll compute hôte**.
  `compute.submit` répond ici avec le résultat terminal. Les sleeps Android
  recouvrent aussi le travail du handler, ils ne sont pas intégralement perdus.
- **Readiness/lease** : état READY et bail ACTIVE avant appel ; zéro acquire de
  bail dans la fenêtre. Le verrou d'opération prend 0,002 ms ; décision de
  placement 0,006 ms. Les 1–2 renouvellements mesurés sont en arrière-plan,
  cumulant 1,492–1,716 s d'attente logicielle chevauchée, pas autant de retard
  synchrone du job. La sérialisation du transport peut occasionner une petite
  attente ; sa part exacte n'est pas identifiée par ces compteurs agrégés.

## Conditions et provenance

Samsung SM-A155F, Android 16/API 36 ; build complet dans le JSON public.
APK isolé `org.phoneboost.app.multicore`, SHA-256
`7ece5cb887986d5408492915489432065c19c352efae19b0c228fef104bf0ce4`. Les octets installés ont été relus et comparés.
Les fichiers source instrumentés sont hachés dans le JSON ; les exécutables et
le `.so` compilé/strippé correspondant sont archivés. Le produit conserve son
SHA-256 `fe4aaedecd63359bdbaeb01a64b962fcc0cee9fa9cba462143f46ca984d48b2a`.

Entrée identique au spike : `byte[i]=(31*i+7)%256`, 64 MiB, BLAKE3
`7b2def9fc609feb94905bd0cdb0772743544d6564b8d73cf2bba66c4759116a4`. Transport authentifié PBMUX/Noise via forward ADB USB,
profil expérimental 2, aucune affinité. Aucun Perfetto, stress, changement de
seuil, de politique ResourceGuard, de contrat, de timeout ou de protocole.

Chaque préflight confirme HAL Ready, absence d'override, USB, Battery Saver OFF,
Health fraîche/NOMINAL, budget 128 MiB, récupération mémoire ≥896 MiB pendant
≥10 s, CPU compatibles. Tous les échantillons pendant les runs restent NOMINAL,
lowMemory=false, thermal=0, et les snapshots natifs encadrent l'admission et la
fin du calcul. Mémoire disponible minimale par run : 947,996 / 1 046,531 /
879,488 MiB. La dernière valeur est sous le seuil de **récupération** 896 MiB,
mais au-dessus du seuil de refus 768 MiB, après récupération acquise ; aucun
contournement de l'hystérésis. Âge Health natif avant/après calcul :
1 157/1 208, 1 279/1 331 et 1 779/1 829 ms. Budget brut 128 MiB ; réservation
buffer+scratch 72 MiB. Aucun fallback, refus ni nouvelle tentative automatique.

Les durées de chaque processus utilisent son horloge monotone locale.
[`Instant` Rust](https://doc.rust-lang.org/1.98.0/std/time/struct.Instant.html)
ne garantit pas une sémantique universelle du suspend ; les durées diagnostic
ne servent jamais à l'admission Health, qui conserve BOOTTIME. La mesure native
existante reste dans son domaine BOOTTIME Android. Aucune époque Android n'est
soustraite à une époque Linux. La capture des compteurs Android encadre le job
avec 195–253 ms de marge maximale de commandes ADB (mesurée côté hôte), qui
peuvent ajouter de l'attente idle ; ses 23,620–25,581 s de `SessionPoll` ne
sont donc pas une durée exclusive et synchronisée du transfert. Les snapshots
atomiques ne sont pas une trace ordonnée de chaque frame.

## Validation et coût de la mesure

Activation par feature Cargo `diagnostic-timing` **et** variable explicite
`PHONEBOOST_TIMING_DIAGNOSTIC=1` sur hôte/CLI. APK :
`scripts/build_multicore_spike_android --timing-diagnostic`, qui exige le build
isolé multicœur ; receiver protégé par `android.permission.DUMP`, commandes
numériques hors protocole. Sans feature, les sondes sont compilées à vide.
Avec feature mais collecte OFF, aucun log diagnostic ni lecture d'horloge de
span. Les métriques ont un ensemble fixe de 59 noms et trois compteurs `u64` ;
aucun contenu, identifiant de session, chemin, peer, clé ou digest n'est ajouté
aux logs de timing. Le stdout/résultat C12 garde son format existant.

- Suites Rust existantes des six crates instrumentées, collecte activée :
  384 tests unitaires/intégration + 6 doctests passent, un microbenchmark ignoré
  par défaut puis exécuté explicitement. Les nombres exacts restent dans les logs.
- Après ajout des sondes réassemblage/parsing : 30 tests + 4 doctests passent.
- `cargo check --locked --workspace --all-targets` sans features passe.
- Clippy selon les options CI passe avec les avertissements déjà présents.
  La tentative locale plus stricte `-D warnings` échoue sur un avertissement
  préexistant de `pb-pbmux` (`manual_range_patterns`) ; aucun nettoyage hors scope.
- Tests C12 par vrais processus existants passent ; le nouveau test OFF/ON
  confirme stdout/codes de retour identiques, fallback explicitement distinct
  de `REMOTE_SUCCESS`, rejet d'entrée identique et métriques numériques bornées.
- APK ARM64 release et `lintDebug` passent. Les sources produit restent isolées
  du receiver diagnostic. La CI compile aussi l'APK diagnostique explicitement.
- Microbenchmark Linux debug, 1 000 000 spans : OFF 30,471 ms, ON 137,617 ms,
  soit **107,146 ns additionnels/span**. 7 050–7 070 spans hôte/run donnent
  environ **0,76 ms estimé** de surcoût unitaire, soit 0,003 % du total. Cette
  extrapolation n'est pas un A/B physique ; contention et coût Android ne sont
  pas isolés. Une seule sortie agrégée par processus évite le log par frame.
- Validation déterministe des preuves :
  `python3 scripts/analyze_c12_timing.py docs/evidence/c12-end-to-end-timing-v0-1.json`.
  Elle vérifie résultat distant, octets, frames, phases imbriquées, Health et résidu.

La fermeture est vérifiée dans le JSON : Battery Saver restauré ON, profil 1,
collecte OFF, APK expérimental arrêté, auto-use du daemon isolé OFF, daemon et
forward possédés supprimés, APK produit inchangé. Aucun worktree historique
n'a été modifié ou supprimé. Les anciens résultats CPU sont conservés.

## Démontré, probable, non mesuré

**Démontré** : 3 résultats distants exacts, goulot dans le chiffrement hôte de
ce build dev, 17 PUT, transport majoritaire en temps logiciel, compute <0,2 %,
import <0,2 % après mise en cache, résidu du chemin critique <0,05 % ; aucune
optimisation multicœur ne peut, seule, supprimer ces secondes de chiffrement.

**Probable mais à tester** : une compilation release de l'hôte réduira
fortement ce coût CPU. Aucun gain release ni débit limite USB n'est mesuré ici.
La contribution précise réseau/ACK/ordonnancement aux ~1,7 s d'attente n'est
pas isolée. Aucun gain produit, coût disque réel ou mérite comparé d'un nouvel
algorithme cryptographique n'est déduit de ce diagnostic.

Comparaison release désormais réalisée : [résultats](C12_RELEASE_VS_DEV_V0_1.md).

**NEXT_ACTION : Réduire la latence du polling de session à 20 ms sur le chemin PUT en release, avec mesure A/B et protocole inchangé.**
