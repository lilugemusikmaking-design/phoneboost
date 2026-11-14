# Spike Android multicœur borné — v0.2

Base : `master` à `edad4ffc90c6143d1a4f43dfa6096be21cb97cfa`, après fusion de #31.
Branche : `spike/android-multicore-v0-2`.
Le SHA contenant ce rapport se retrouve par `git log -1 --format=%H -- docs/architecture/ANDROID_MULTICORE_SPIKE_V0_2.md`.

## Périmètre et exception explicite

L'utilisateur a autorisé une exception **exclusivement expérimentale** pour les
profils explicites 1/2/4. C03/C08/C10, les cinq sources canoniques, C13, les seuils
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
40 s par profil se termine naturellement. La fenêtre initiale fournie de 20 s
était trop courte pour le chemin complet mesuré à 29,106 s ; elle est conservée
comme tentative incomplète. La fenêtre finale inclut démarrage, attente Health
et une marge bornée fondée sur cette mesure. L'analyse utilise explicitement
`trace_processor query --format csv`. Le filtre conserve le package expérimental
et les noms `pb-cpu-*`, puis intersecte les slices Running avec les TID et
intervalles BOOTTIME du job. Cela exclut le trafic IPC du worker 0 et les sondes
de disponibilité. Elle rapporte séparément threads observés, CPU utilisés,
simultanéité maximale, temps avec ≥2/≥3/≥4 Running, CPU time et migrations entre
deux slices successives du même TID. Les erreurs de trace, sa couverture et
l'alignement BOOTTIME sont contrôlés avant qualification.

La campagne impose HAL thermique Ready sans override, même entrée et digest
exact, un warmup puis six répétitions par profil admissible en ordre équilibré.
Un refus arrête les tentatives de ce profil, sans réglage destiné à le forcer.
Pour confirmer le multicœur : Perfetto doit démontrer un overlap Running réel
sur plusieurs CPU dans une trace complète sans perte détectée. Les compteurs
natifs restent une vérification complémentaire : au moins cinq runs sur six
montrent au moins deux threads/CPU et somme CPU / wall-clock > 1,10. Pour conclure
à un bénéfice matériel : accélération médiane appariée ≥ 1,10 et au moins cinq
paires sur six > 1,05. Un digest incorrect interdit tout verdict bénéfique.

## Validation et résultat

La validation logicielle et la preuve physique sont consignées séparément dans
`docs/evidence/android-multicore-v0-2.json`.

**Verdict : `MULTICORE_BLOCKED_BY_RESOURCE_GUARD`.** Le Samsung SM-A155F,
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

La PR dédiée doit passer Actions/CodeQL et reste ouverte en attendant une vraie
exécution multicœur sur Android. Le blocage unique est l'absence de conditions
Health admissibles pour ce test ; ce n'est pas une remise en cause de la source
Health qualifiée par #31. L'amélioration de l'ergonomie appairage/verrouillage
est explicitement différée à la demande de l'utilisateur.

**NEXT_ACTION : rejouer la preuve physique Perfetto 1/2/4 lorsque Health est
naturellement nominale, sans modifier les seuils ResourceGuard.** RAM/GPU/NPU
restent fermés.

Les tests ne prouvent pas la performance d'un téléphone. Aucune campagne WebP,
CAPACITY/THROUGHPUT historique, audit SoC ou étude below-OS n'est relancée.
Le collecteur Health quarantiné et les worktrees historiques restent intacts.

Références de l'implémentation : [BLAKE3 hazmat 1.8.6](https://docs.rs/blake3/1.8.6/blake3/hazmat/index.html),
[permission Android DUMP](https://developer.android.com/reference/android/Manifest.permission#DUMP),
[qualification Health intégrée](ANDROID_HEALTH_SOURCE_V0_1.md).
