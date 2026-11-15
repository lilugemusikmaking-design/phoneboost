# Spike Android multicœur borné — v0.2

Base : `master` à `edad4ffc90c6143d1a4f43dfa6096be21cb97cfa`, après fusion de #31.
Branche : `spike/android-multicore-v0-2`.
Le SHA contenant ce rapport se retrouve par `git log -1 --format=%H -- docs/architecture/ANDROID_MULTICORE_SPIKE_V0_2.md`.

## État final : profil 4 qualifié, profil 2 retenu pour le compromis actuel

Le spike CPU est fermé sur la décision expérimentale suivante : **profil 4
réellement exécuté sur quatre CPU simultanés, gain supplémentaire non qualifié
face au profil 2**. Le verdict global reste **MULTICORE_CONFIRMED_BENEFICIAL**,
fondé sur la comparaison 1→2 déjà qualifiée (1,754× médian apparié, 6/6 favorables).
Le produit reste à un thread. Aucun gain produit PhoneBoost n'est démontré.
Les [preuves 2→4](../evidence/android-multicore-comparison-2-4-v0-2.json) contiennent
tous les résultats, y compris le run isolé et l'arrêt du dernier préflight.

Même APK exact `org.phoneboost.app.multicore`, même entrée 64 MiB et oracle,
même transport USB/ADB authentifié. Avant chaque job : HAL Ready sans override,
Battery Saver OFF, Health fraîche et nominale, mémoire récupérée ≥896 MiB/10 s,
budget suffisant et CPU autorisés 0–7. Aucune affinité ni protection modifiée.

### Preuve scheduler du profil 4

Une seule capture complète, banc **16 MiB / 55 s inchangé**, arrêt naturel.
REMOTE_SUCCESS, digest exact, Health nominale avant/pendant/après. Le maximum
simultané provient des véritables slices Running bornées aux fenêtres BOOTTIME/TID,
recoupées par balayage temporel indépendant et intersections sur CPU distincts.

| Mesure de la capture | Profil 4 |
| --- | --- |
| TID `pb-cpu-*` | 19385, 19386, 19387, 19388 |
| CPU logiques observés | 0, 1, 2, 3, 5, 6, 7 |
| Maximum Running simultané | 4 |
| Temps avec ≥2 / ≥3 / ≥4 Running | 44,003153 / 26,999770 / 25,824309 ms |
| Wall-clock PhoneBoost | 45,968077 ms |
| CPU time scheduler / natif | 141,505001 / 140,075305 ms |
| Migrations réelles | 11 |
| Mémoire minimale / budget minimal pendant ce job | 980,676 / 117,169 MiB |

Les 133 blocs et 4 358 144 octets écrits dans le buffer ont tous été lus.
Aucune perte qualifiante détectée dans la session, aucune erreur ftrace/parser,
ni rupture entre paquets de même séquence. Les marqueurs initiaux de perte portent tous le drapeau explicite
`first_packet_on_sequence`, selon le
[schéma Perfetto v51.2](https://github.com/google/perfetto/blob/v51.2/protos/perfetto/trace/trace_packet.proto#L326).
Le compteur global de service est passé de 3 hier à 4 aujourd'hui ; son origine
et son instant d'incrément restent inconnus. Ce n'est pas un delta de cette
session. Le premier audit opérateur a donc arrêté la qualification, puis les
compteurs propres à la trace et sa continuité ont été vérifiés. Aucune nouvelle
capture, aucun filtre supprimé, aucun résultat perdu transformé en succès.

### Comparaison 2→4 et arrêt borné

Plan annoncé : warmup 4 avec trace, warmup 2 sans trace, puis six paires alternant
2/4 et 4/2. Réalisé : **cinq paires complètes + le profil 4 de la sixième**.
Les 13 jobs soumis sont distants, exacts et admissibles. Le dernier profil 2 n'a
jamais été soumis. Il n'y a ni nouvelle tentative ni remplacement sélectif.

| Médiane des cinq paires complètes | Profil 2 | Profil 4 |
| --- | --- | --- |
| Wall-clock natif | 51,155 ms | 48,310 ms |
| CPU time natif, somme des feuilles | 97,964 ms | 136,911 ms |
| Débit natif | 1251,098 MiB/s | 1324,788 MiB/s |
| End-to-end CLI | 27,499 s | 27,686 s |
| Mémoire minimale pendant les jobs | 1030,684 MiB | 1013,695 MiB |
| Budget minimal pendant les jobs | 128,000 MiB | 125,424 MiB |

Ratios natifs appariés : 0,8171 / 1,0654 / 1,0150 / 1,1327 / 1,1420.
Médiane **1,065364×**, trois paires sur cinq >1,05×. Le critère existant exige
≥1,10× médian et ≥5/6 paires >1,05×. Même favorable, la paire manquante n'aurait
porté ce compte qu'à 4/6. Le gain matériel supplémentaire n'est donc pas qualifié.
Le CPU time médian augmente d'environ 40 %. Le sixième profil 4, à **69,703308 ms**
(CPU time 187,673994 ms), est conservé séparément ; médiane des six profils 4
mesurés : **49,736770 ms**. Aucun ancien profil 2 ne lui est associé artificiellement.

Le ratio end-to-end apparié médian est **0,986309×**, aucune paire >1,05×.
Aucun effet favorable reproductible n'est établi. La campagne mesure seulement
cette latence ; sa décomposition reste le prochain chantier. Les mesures CLI
excluent les probes opérateur ; leurs durées englobantes sont conservées aussi.
La somme CPU des feuilles ne constitue pas une mesure d'énergie.

Pendant les jobs : lowMemory=false, charging=true, powerSave=false, thermique
officiel NONE, Health fraîche ; âge source maximal à la fin compute des paires
≤2034 ms. Batterie HAL 27,7–28,0 °C, skin 31,3–31,5 °C aux préflights appariés.
Le dernier profil 4 reste admissible malgré un minimum de **884,340 MiB**.

Après ce dernier job, la mémoire système descend à **737,020 MiB** : C08 passe en
refus mémoire, budget nul, tout en conservant lowMemory officiel=false. L'origine
de cette baisse n'est pas attribuée au profil 4 seul. À l'échéance des 120 s,
**C08 a récupéré** : dernier échantillon nominal, 1124,355 MiB et budget 128 MiB.
Le banc exige toutefois sept échantillons consécutifs déjà nominaux (environ
12 s) ; seuls les deux derniers le sont. C'est ce contrôle opérateur plus
conservateur qui arrête le dernier préflight, pas un refus C08 encore actif.
Cette limite du banc est conservée explicitement ; elle ne vaut ni six paires
complètes, ni preuve d'un défaut ResourceGuard. Aucun seuil n'est modifié.

Le profil 2 reste le meilleur compromis expérimental établi sur cet appareil et
ce workload. Cela ne prouve pas une absence universelle de gain à quatre threads.
Le verdict global positif de 1→2 reste intact, sans nouvelle taxonomie globale.

### Clôture

Battery Saver réactivé manuellement et vérifié dans Android et Health fraîche ;
profil 1 restauré, auto-use OFF, hôte/tunnel privés absents, APK expérimental
arrêté, deux APK inchangés. Les preuves et scripts opérateur sont archivés avec
hashes. Les binaires hôte ont été reconstruits depuis le même code via le cache
existant après nettoyage de `/tmp` ; l'APK exact n'a pas été réinstallé.

La PR32 reste **draft et non fusionnée**, selon la demande utilisateur. Les
[preuves 1→2](../evidence/android-multicore-comparison-1-2-v0-2.json) et les refus
antérieurs restent historiques ; leurs anciens NEXT_ACTION sont remplacés ici.
C03/C08/C10/C13, ResourceGuard, plafonds mémoire et produit inchangés.

**NEXT_ACTION : Diagnostiquer quantitativement les ~28 s end-to-end en décomposant C12 import, transport/PBMUX, copies, readiness/lease et compute, avant toute nouvelle optimisation.**

## Périmètre et exception explicite

L'utilisateur a autorisé une exception **exclusivement expérimentale** pour les
profils explicites 1/2/4 dans l’implémentation. Les campagnes physiques ont
qualifié 1→2 puis l'exécution du profil 4 ; la comparaison 2→4 s'arrête selon le
préflight borné décrit ci-dessus. C03/C08/C10, les cinq sources canoniques, C13, les seuils
Health et les plafonds mémoire ne sont pas modifiés. `NATIVE_OPERATION_THREADS`
reste à 1. Aucun choix dynamique du nombre de threads, aucune affinité CPU,
aucun nouveau workload, aucune RAM distante résidente, aucun GPU/NPU.

Le produit ne compile pas la feature Cargo `experimental-multicore`. L'APK
expérimental exige aussi `-PphoneboostMulticoreSpike=true`, possède l'identité
distincte `org.phoneboost.app.multicore`, ses sources et bibliothèques séparées,
et ne propose aucune variante release. Le pipeline produit refuse une bibliothèque
contenant les exports JNI expérimentaux. La sélection locale est limitée à un
receiver exigeant la permission système `android.permission.DUMP`; aucune
commande PBMUX ne choisit un profil. Ce receiver est absent du produit.

## Audit du chemin réel

```text
phoneboostctl compute blake3 --stdin (fichier régulier)
  -> import C12 -> AutoUseController -> session Noise/PBMUX authentifiée
  -> AndroidAuthenticatedCommandHandler -> JNI with_worker (mutex global)
  -> WorkerCore.apply_compute_request / apply_compute_submit
  -> lease + RemoteBuffer READY même session + réservation scratch C08
  -> ResourceGuard.admit_cpu_profile (feature expérimentale uniquement)
  -> BLAKE3 borné, entrée empruntée sans copie complète
  -> jointure de tous les threads -> résultat exact C10 / libération scratch
```

Le mutex JNI sérialise encore les jobs, les mises à jour Health et les commandes
de contrôle. Le parallélisme se trouve **dans un seul job**, pas entre plusieurs
jobs. Health conserve le chemin Android officiel de #31 et l'horloge BOOTTIME.
Pendant le job, l'âge du dernier échantillon continue d'augmenter ; aucune mesure
synthétique ne le rafraîchit. Une péremption entraîne l'abandon aux limites de
quantum, sans résultat partiel.

Le fournisseur produit était un Hasher BLAKE3 séquentiel, avec vérification de
session et de durée tous les 1 MiB. Le `ComputeCancel` entrant reste sérialisé par
le mutex : cette mission n'ajoute pas d'annulation réseau asynchrone. La révocation
atomique de session, l'expiration de lease, la péremption Health et le timeout sont
vérifiés avant/après chaque quantum dans le chemin expérimental. Tous les enfants
sont joints avant de libérer la réservation, y compris en erreur ou panic.

## Architecture minimale et admission

BLAKE3 reste verrouillé à **1.8.6**, sans Rayon ni nouvelle dépendance résolue.
L'API `hazmat` versionnée permet de calculer les sous-arbres avec leurs offsets
exacts, puis de fusionner leurs chaining values avec le bon traitement racine.
Les coupures suivent `left_subtree_len`, et non une division arbitraire en octets.
Les tests comparent le résultat au BLAKE3 standard, y compris autour des limites
de chunks et des puissances de deux. Le profil 1 utilise le même Hasher standard.

Des threads scoped sont créés pour le sous-arbre gauche ; le thread courant
traite le droit. Au maximum N threads existent dans le calcul pour le profil N,
avec au maximum N−1 piles additionnelles demandées de 512 KiB. Pas de pool global,
pas de thread de calcul persistant après retour. Les petites entrées peuvent
utiliser moins de threads que le profil demandé. Le scheduler Android choisit
le placement. Les threads portent les noms déterministes `pb-cpu-0` à
`pb-cpu-3` ; le thread de commande existant est le worker 0, les autres sont
scoped. `sched_getaffinity` est une **lecture** de disponibilité ; aucune
affinité n'est définie.

| Condition | Profil 1 | Profils 2/4 |
| --- | --- | --- |
| Health indisponible, périmé ou refusé | refus | refus |
| THROTTLE : Battery Saver, thermique MODERATE ou batterie basse selon C08 | autorisé si les autres conditions passent | refus |
| Bande NOMINAL, CPU autorisés suffisants | autorisé | autorisé sous réservation |
| CPU disponibles inconnus/insuffisants | refus | refus |
| Scratch | contrat C10 existant | exactement l'enveloppe existante 8 MiB |

L'hôte expérimental réserve les 8 MiB existants ; l'hôte produit reste à 1024
octets. Le plafond global reste 128 MiB et la classe scratch reste plafonnée à
8 MiB. L'entrée physique de 64 MiB plus scratch représente 72 MiB réservés.
Les piles, hashers et diagnostics bornés sont inclus dans cette enveloppe ;
le RSS global du processus et les allocations du runtime Android ne sont pas
assimilés à ce compteur de réservation.

Un refus avant consommation laisse la réservation au propriétaire appelant,
qui doit la libérer. Un job consommé libère le scratch une seule fois après
jointure. Les tests JNI exercent ce comportement sur le vrai canal authentifié.

## Instrumentation et critère annoncé avant mesure

Une seule trace de workload/refus est retenue : profil, octets, wall-clock natif,
temps CPU par thread, TID Linux, intervalle de calcul, logical CPUs échantillonnés
et migrations observées, réservations avant/après, décision et Health avant/après.
Une sonde de disponibilité minuscule réussie n'efface pas une trace de workload.
Le dump est diagnostique, jamais une autorité d'admission ; cohérence de séquence
et identifiant de probe empêchent d'utiliser une ancienne ligne logcat.

La wall-clock native BOOTTIME couvre création et jointure. La somme CPU couvre le travail
des feuilles et sous-estime donc légèrement le coût total. Les CPU sont observés
aux limites des quanta : les migrations non échantillonnées restent inconnues.
Le chevauchement des scopes n'est pas une preuve de simultanéité matérielle.
La latence complète C12 est rapportée séparément et inclut les transferts.

Le banc fourni par l'utilisateur dans `phoneboost_perfetto_bench/` reste la
source de la capture et du calcul SQL `intervals_overlap_count!`. Une trace de
55 s par profil se termine naturellement. Les anciennes fenêtres de 20 s puis
40 s ont manqué le compute de contrôles exacts et restent conservées comme
incomplètes. La durée de 55 s a été autorisée sur la mesure de 46,921 s du début
de capture à la fin du compute ; elle inclut une marge bornée de 8,079 s. L'analyse utilise explicitement
`trace_processor query --format csv`. Le filtre conserve le package expérimental
et les noms `pb-cpu-*`, puis intersecte les slices Running avec les TID et
intervalles BOOTTIME du job. Cela exclut le trafic IPC du worker 0 et les sondes
de disponibilité. Elle rapporte séparément threads observés, CPU utilisés,
simultanéité maximale, temps avec ≥2/≥3/≥4 Running, CPU time et migrations entre
deux slices successives du même TID. Les erreurs de trace, sa couverture et
l'alignement BOOTTIME sont contrôlés avant qualification.

La campagne comparative impose HAL thermique Ready sans override, même entrée
et digest exact, un warmup puis six répétitions par profil admissible en ordre
équilibré. La campagne 1/2 historique satisfait ces critères.
La comparaison 2/4 décrite en tête conserve son arrêt à cinq paires complètes et
son sixième profil 4 isolé ; aucun gain incrémental qualifié n'en est déduit.
Un refus arrête les tentatives de ce profil, sans réglage destiné à le forcer.
Pour qualifier le multicœur reproductible : Perfetto doit démontrer un overlap Running réel
sur plusieurs CPU dans une trace complète sans perte détectée. Les compteurs
natifs restent une vérification complémentaire : au moins cinq runs sur six
montrent au moins deux threads/CPU et somme CPU / wall-clock > 1,10. Pour conclure
à un bénéfice matériel : accélération médiane appariée ≥ 1,10 et au moins cinq
paires sur six > 1,05. Un digest incorrect interdit tout verdict bénéfique.

## Historique : validation et refus de la première campagne

La validation logicielle et la preuve physique sont consignées séparément dans
`docs/evidence/android-multicore-v0-2.json`.

**Verdict historique : `MULTICORE_BLOCKED_BY_RESOURCE_GUARD`.** Le Samsung SM-A155F,
Android 16/API 36, build `BP4A.251205.006.A155FXXSBEZG1`, a exécuté exactement
l'APK issu du checkpoint logiciel `93f1de187838163729c476da2c327f5a612cf8a9`.
Son SHA-256 installé correspond au fichier construit. L'APK produit est resté
strictement identique avant/après ; seul le package expérimental a été installé.

| Tentative physique, entrée identique 64 MiB | Résultat | Compute natif | Preuve scheduler |
| --- | --- | --- | --- |
| 1, premier contrôle | REMOTE_SUCCESS, digest exact | 1 TID, CPU échantillonné 6, 120,456 ms, 531,315 MiB/s ; CPU time 119,429 ms | Trace de 20 s terminée avant le calcul : non qualifiée |
| 1, capture complète 40 s | RESOURCE_REFUSED, fallback hôte explicite | Aucun job C10 admis | Aucune fenêtre compute qualifiée |
| 2, capture complète 40 s | RESOURCE_REFUSED, fallback hôte explicite | Aucun job C10 admis | Aucune fenêtre compute qualifiée |
| 4, capture complète 40 s | RESOURCE_REFUSED, fallback hôte explicite | Aucun job C10 admis | Aucune fenêtre compute qualifiée |

Le premier contrôle prend **29,106 s de bout en bout**. Son débit natif isolé
n'est ni une médiane ni une preuve de gain. Les digests des tentatives refusées
proviennent explicitement du fallback local : ils ne prouvent pas l'équivalence
physique des profils 2/4. Les tests logiciels prouvent cette équivalence.

La capture complète du profil 1 contient quatre lectures mémoire sous 768 MiB,
puis un refus avec budget nul. Dans les fenêtres BOOTTIME des profils 2 et 4,
respectivement **20/20 et 19/19** lectures Health officielles indiquent
`RefusedMemory` et budget zéro. Leur âge maximal est 6/8 ms : c'est un refus
mémoire frais, pas une ancienne mesure nominale. L'hystérésis 896 MiB/10 s reste
en vigueur. `lowMemory` officiel n'est pas assimilé à ces seuils internes.
Battery Saver reste actif, en charge, ce qui empêche aussi la bande NOMINAL
requise par les profils expérimentaux >1. Leur admission CPU propre n'a pas été
atteinte ; on ne transforme pas le refus C08 en un test physique de ce prédicat.

HAL thermique Ready, AIDL 3, aucun override, statut Android/PhoneBoost 0 avant
et après. Aucun stress thermique ni réglage système changé. Les traces sont
complètes, alignées BOOTTIME et sans compteur de perte/erreur détecté ; leurs
agrégats compute à zéro signifient **aucun intervalle admis**, pas une exécution
multicœur mesurée à zéro. Les résumés automatiques de performance restent donc
`NOT_CONFIRMED` ; le verdict opérationnel `BLOCKED_BY_RESOURCE_GUARD` est établi
en croisant le refus C12 explicite avec les fenêtres Health enregistrées.

Les répétitions de performance supplémentaires n'ont pas été lancées après ces
refus. Aucun réglage, seuil ou collecte Health n'a été contourné. Le buffer
Perfetto configuré à 64 MiB et l'import réel ont un coût mémoire non isolé :
cette campagne ne prouve pas qu'un téléphone non instrumenté aurait la même
admission. Le profil 1 a été restauré, l'hôte privé désactivé, le seul tunnel
ADB créé retiré et l'application expérimentale arrêtée. Le produit reste intact.

À ce stade historique, la qualification de performance restait à faire et la PR
devait passer Actions/CodeQL. Le blocage de cette campagne était l'absence de conditions
Health admissibles pour ce test ; ce n'est pas une remise en cause de la source
Health qualifiée par #31. L'amélioration de l'ergonomie appairage/verrouillage
est explicitement différée à la demande de l'utilisateur.

Cette ancienne indication de reprise 1/2/4 est remplacée par la clôture physique
2/4 et son NEXT_ACTION en tête de ce rapport. Les refus historiques restent des
observations valides de leurs conditions de test.

Les tests ne prouvent pas la performance d'un téléphone. Aucune campagne WebP,
CAPACITY/THROUGHPUT historique, audit SoC ou étude below-OS n'est relancée.
Le collecteur Health quarantiné et les worktrees historiques restent intacts.

Références de l'implémentation : [BLAKE3 hazmat 1.8.6](https://docs.rs/blake3/1.8.6/blake3/hazmat/index.html),
[permission Android DUMP](https://developer.android.com/reference/android/Manifest.permission#DUMP),
[qualification Health intégrée](ANDROID_HEALTH_SOURCE_V0_1.md).
