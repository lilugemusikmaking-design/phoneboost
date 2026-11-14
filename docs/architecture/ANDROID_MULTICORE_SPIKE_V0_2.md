# Spike Android multicœur borné — v0.2

Base : `master` à `edad4ffc90c6143d1a4f43dfa6096be21cb97cfa`, après fusion de #31.
Branche : `spike/android-multicore-v0-2`.
Le SHA contenant ce rapport se retrouve par `git log -1 --format=%H -- docs/architecture/ANDROID_MULTICORE_SPIKE_V0_2.md`.

## État actuel : preuve scheduler profils 1 et 2

Verdict intermédiaire : **MULTICORE_EXECUTION_CONFIRMED**. Le profil 2 a réellement
exécuté deux threads simultanément sur des CPU logiques distincts. Cette preuve
ne qualifie ni la reproductibilité, ni un gain de performance, ni un avantage produit.
Les [preuves détaillées](../evidence/android-multicore-scheduler-1-2-v0-2.json)
supplantent le refus historique conservé plus bas. Le profil 4 n'a pas été lancé.

Même APK `org.phoneboost.app.multicore`, entrée 64 MiB, oracle BLAKE3 indépendant
et transport USB/ADB authentifié. Battery Saver a été désactivé manuellement par l’utilisateur pour
les essais. Chaque préflight vérifie le HAL Ready sans override, USB branché,
Health fraîche, bande nominale, budget suffisant et récupération naturelle
896 MiB/10 s. Une seconde lecture avant import refuse la soumission si l'état
n'est plus admissible. Aucune politique ni source Health n'est modifiée.

| Mesure physique | Profil 1 | Profil 2 |
| --- | --- | --- |
| Mémoire / budget au préflight (MiB) | 1076,6 / 128 | 1044,0 / 128 |
| Âge effectif au préflight (ms) | 235 | 799 |
| TID compute | 30849 | 30968, 30969 |
| CPU logiques utilisés | 3, 6 | 5, 6, 7 |
| Maximum Running simultané | 1 | 2 |
| Temps avec >=2 Running (ms) | 0 | 49,913692 |
| Temps CPU scheduler (ms) | 102,436693 | 102,609692 |
| Wall-clock PhoneBoost (ms) | 102,739154 | 58,836385 |
| Temps CPU natif PhoneBoost (ms) | 101,719151 | 101,757769 |
| Migrations sched réelles | 1 (3→6) | 1 (5→6) |
| Résultat distant exact | oui | oui |
| Trace complète, sans erreur/perte détectée | oui | oui |
| ResourceGuard pendant le job | nominal | nominal |

Le profil 1 utilise deux CPU successivement : il reste mono-thread. Pour le
profil 2, le TID30968 court sur CPU7 pendant que TID30969 court sur CPU5 puis6.
L'intersection des véritables slices Running vaut49,913692ms ; une pause de82,231µs
entre les slices du second TID est exclue. Le calcul est borné aux fenêtres
BOOTTIME/TID natives et vérifié indépendamment des agrégats SQL. Les wall-clock et
digests viennent de PhoneBoost. Aucune comparaison statistique n'est déduite de
ces deux essais isolés ni de leurs durées de transfert variables.

Une tentative a été arrêtée avant soumission par un problème du contrôleur ADB
qui héritait de l'entrée stdin ; les probes utilisent maintenant DEVNULL sans
consommer le fichier de calcul. Un autre contrôle distant exact a terminé après
la trace40s : il est conservé comme non qualifié. L'utilisateur a ensuite autorisé
**uniquement la durée40→55s**, sur la mesure46,921s jusqu'à la fin compute. Buffer
65536KiB, événements, SQL et protections PhoneBoost restent inchangés. Les deux
captures55s finissent naturellement ; aucun refus mémoire n'est survenu.

Fin de mesure : profil expérimental remis à 1, auto-use OFF, daemon et tunnel
privés arrêtés. Le retour de Battery Saver à ON reste en attente d’une action
manuelle : la commande native a renvoyé 0 mais Health est restée OFF. Le service
Health expérimental reste actif pour vérifier cette restauration ; aucun workload
n’est en cours. Aucun état de restauration réussi n’est déclaré sans observation.

La PR32 reste draft et non fusionnée. **NEXT_ACTION : effectuer la comparaison
physique reproductible 1 vs 2 ; profil 4 seulement ensuite.** RAM/GPU/NPU et toute
extension C13 restent fermés.

## Périmètre et exception explicite

L'utilisateur a autorisé une exception **exclusivement expérimentale** pour les
profils explicites 1/2/4 dans l’implémentation. La campagne présente reste
strictement limitée à 1/2 ; elle ne lance pas le profil 4. C03/C08/C10, les cinq sources canoniques, C13, les seuils
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

La campagne comparative prévue impose HAL thermique Ready sans override, même
entrée et digest exact, un warmup puis six répétitions par profil admissible en
ordre équilibré. La mission présente se limite à une capture qualifiée par
profil 1/2 ; elle ne satisfait pas encore ces critères de reproductibilité.
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

La PR dédiée doit passer Actions/CodeQL et reste ouverte pour la qualification
de performance encore manquante. Le blocage de cette campagne historique était
l'absence de conditions
Health admissibles pour ce test ; ce n'est pas une remise en cause de la source
Health qualifiée par #31. L'amélioration de l'ergonomie appairage/verrouillage
est explicitement différée à la demande de l'utilisateur.

Cette ancienne indication de reprise 1/2/4 est remplacée par la preuve scheduler
1/2 et son NEXT_ACTION en tête de ce rapport. Les refus historiques restent des
observations valides de leurs conditions de test.

Les tests ne prouvent pas la performance d'un téléphone. Aucune campagne WebP,
CAPACITY/THROUGHPUT historique, audit SoC ou étude below-OS n'est relancée.
Le collecteur Health quarantiné et les worktrees historiques restent intacts.

Références de l'implémentation : [BLAKE3 hazmat 1.8.6](https://docs.rs/blake3/1.8.6/blake3/hazmat/index.html),
[permission Android DUMP](https://developer.android.com/reference/android/Manifest.permission#DUMP),
[qualification Health intégrée](ANDROID_HEALTH_SOURCE_V0_1.md).
