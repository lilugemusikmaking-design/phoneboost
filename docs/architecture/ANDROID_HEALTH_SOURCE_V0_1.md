# Source Android health minimale — v0.1

Base : `master` à `251306e825db79173f0bdeb97d1823d08d894ca7`.
Branche : `fix/android-health-source-v0-1`.
Le commit contenant ce rapport se retrouve avec
`git log -1 --format=%H -- docs/architecture/ANDROID_HEALTH_SOURCE_V0_1.md`.

Le producteur Android public existait déjà. Cette correction en précise la
sémantique et ferme les défauts de fraîcheur et d'indisponibilité ; elle ne
réactive pas le collecteur shell quarantiné. Qualification documentaire et
logicielle uniquement. **Aucune preuve physique nouvelle, aucun AVD exécuté.**
Le téléphone est indisponible pour cette mission, sur confirmation de l'utilisateur.
Le chantier logiciel continue avec les fixtures et la vraie liaison JNI sur hôte.

## Autorité et périmètre

Les cinq sources canoniques de `.pipeline/AUTHORITY.json` ont été retrouvées et
leurs empreintes comparées aux empreintes enregistrées : SPEC V0.7, Contract Set
V1.3, Tech Sheet V1.3, Pseudocode V1.1 et Fixture Generation Spec V1.0. Aucun
changement canonique. C08/RG012 restent l'autorité : intervalle 2 s, péremption
au-delà de 6 s, mémoire 768/896 MiB, marge 512 MiB, plafond 128 MiB, seuils
thermiques et batterie et durées de récupération inchangés.

Ce travail porte sur l'entrée health et sa validation. Les modifications des
fichiers de tests compute, remote_buffer et host adaptent uniquement leurs
fixtures : une période de récupération comporte désormais des lectures
intermédiaires fraîches. Aucun fournisseur, workload, protocole, nombre de
threads, comportement RAM distante, GPU/NPU ou invariant d'autorité n'est ouvert.

## Audit du chemin effectif

```text
PhoneBoostService / HandlerThread (2 s, sans wake lock)
  -> workerHealthSampleTimeMs() : début de collecte dans l'horloge du worker
  -> Context.readAndroidObservations() : API publiques Android
  -> AndroidObservations.publishHealth() : cohérence et complétude
  -> WorkerNative.workerUpdateHealth() / workerInvalidateHealth()
  -> JNI update_health() : types, domaines, flags et invalidation
  -> WorkerCore.record_local_health() : âge dans l'horloge native
  -> ResourceGuard.record_health() / invalidate_health()
  -> reserve et commit : décision d'admission sous lease authentifiée
```

`MainActivity` lit aussi `AndroidObservations`, pour l'affichage seulement.
Les autres producteurs Rust sont des fixtures de tests. Le heartbeat C08 dérive
ses valeurs du worker sous lease authentifiée ; aucune observation distante
ne peut écrire la santé locale. Les consommateurs host lisent cette télémétrie,
mais l'admission effective demeure dans ResourceGuard. Les fournisseurs consomment
les réservations validées : ce patch n'introduit ni préemption de jobs engagés
ni nouveau contrôle de ressources après commit.

Les champs qui influencent l'admission sont `available_memory_bytes`, `low_memory`,
`thermal`, `battery_percent`, `charging`, `power_save` et la fraîcheur. `budget_bytes`
est calculé avec la formule canonique ; ce n'est pas une mesure Android. Les bandes
thermique/batterie effectives peuvent conserver un refus pendant l'hystérésis.
Le label thermique et les MiB de l'UI sont des représentations dérivées.

L'absence de dernier échantillon donne `RefusedStaleState`, bandes UNKNOWN, âge
absent et budget zéro. Les autres valeurs sentinelles de `HealthStatus::unavailable`
(dont `low_memory=true`) ne sont **pas** des observations Android ; le heartbeat
est supprimé lorsque la santé est indisponible/périmée. `headroom_present=0`
reste explicitement absent ; les compteurs C08 actuellement à zéro ne participent
pas à l'admission health. La lecture JNI champ par champ est diagnostique,
non atomique, et ne constitue pas une autorité d'admission.

## Sources retenues et limites exactes

Toutes les API ci-dessous sont disponibles au `minSdk=29`, sans nouvelle
permission de lecture. Leur coût n'a pas été mesuré sur appareil pendant cette
mission. Les coûts indiqués décrivent les appels, pas un benchmark.

| Signal | Source / API minimale | Coût et fraîcheur observables | Déduction légitime de ResourceGuard | Déduction interdite |
| --- | --- | --- | --- | --- |
| Mémoire disponible | `ActivityManager.getMemoryInfo().availMem`, API 1 | Une requête mémoire par collecte | Appliquer la marge et les seuils C08 | Garantie d'allocation, RAM libre physique, pression PSI ou budget par processus |
| Cohérence mémoire | `MemoryInfo.totalMem`, API 16, dans la même requête | Aucun appel supplémentaire ; valide `0 <= availMem <= totalMem` et `totalMem > 0` | Refuser un tuple impossible ; zéro disponible est valide et refusé pour mémoire | Définir une nouvelle capacité produit à partir de la RAM annoncée |
| État mémoire bas | `MemoryInfo.lowMemory`, API 1 | Même réponse que `availMem` | Refus si Android déclare cet état | Assimiler un niveau mémoire interne, un trim de processus ou un seuil reconstruit à ce booléen |
| Thermique | `PowerManager.currentThermalStatus`, API 29 | Une seule lecture, partagée par code et label | Appliquer les bandes Android 0–6 ; MODERATE limite, SEVERE+ refuse | Température CPU, marge en watts, budget de cœurs, preuve d'un capteur fonctionnel à partir de NONE |
| Niveau batterie | Sticky `ACTION_BATTERY_CHANGED`, `EXTRA_LEVEL/SCALE` (API 5) | Une lecture du dernier état publié ; aucune date de mesure matérielle exposée par ce tuple | Pourcentage entier si `0 <= level <= scale`, calcul en Long | Temps restant, puissance ou garantie de fraîcheur du HAL batterie |
| Charge | `EXTRA_STATUS`, API 5 | Même Intent | CHARGING/FULL vrais ; DISCHARGING/NOT_CHARGING faux ; UNKNOWN/absent invalides | Déduire un débit de charge ou traiter une absence comme faux |
| Battery Saver | `PowerManager.isPowerSaveMode`, API 21 | Une lecture d'état système | THROTTLE selon le contrat actuel | Low Power Standby, Doze, écran éteint ou garantie d'exécution en arrière-plan |

Les définitions mémoire viennent de la [référence MemoryInfo](https://developer.android.com/reference/android/app/ActivityManager.MemoryInfo).
Le calcul AOSP de `lowMemory` est vérifié dans
[ProcessList, Android 16 r1](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-16.0.0_r1/services/core/java/com/android/server/am/ProcessList.java).
On conserve le booléen officiel ; on ne le recalcule pas à partir de `threshold`.
Les états batterie et économie d'énergie suivent les références
[BatteryManager](https://developer.android.com/reference/android/os/BatteryManager) et
[PowerManager](https://developer.android.com/reference/android/os/PowerManager).

**Écran éteint / arrière-plan :** ces lectures ne deviennent pas d'autres
métriques. Elles restent accessibles si le processus s'exécute ; le foreground
service ne garantit pas une cadence pendant la veille profonde ni après sa mort.
Le Handler peut être retardé. Une interruption laisse la santé périmer, sans
ajout de réveil, permission ou boucle de polling. Ces comportements sont justifiés
par le contrat d'API, mais n'ont pas été observés sur le téléphone dans cette PR.

**Limite thermique explicite :** AOSP Android 16 initialise `mStatus` à NONE et
`getCurrentThermalStatus()` peut le retourner même si le HAL n'est pas prêt.
Une exception ou un code invalide est donc détectable ; un NONE plausible ne
prouve pas à lui seul la disponibilité matérielle. Cette limite est vérifiée dans
[ThermalManagerService, Android 16 r1](https://android.googlesource.com/platform/frameworks/base/+/refs/tags/android-16.0.0_r1/services/core/java/com/android/server/power/ThermalManagerService.java).
La qualification du téléphone doit encore vérifier cette provenance. Le code
n'invente pas une disponibilité HAL à partir d'une valeur synthétique.

## Horloges, indisponibilité et refus

Avant correction, Java horodatait après lecture, puis le worker remplaçait cet
horodatage par l'heure de réception. Une lecture lente devenait artificiellement
récente. Le worker Android utilisait aussi `std::time::Instant`, adossé à
`CLOCK_MONOTONIC` dans la source installée Rust 1.98 : la veille profonde n'entrait
pas dans l'âge. Les timestamps Android passés à la lecture JNI restent sans
autorité, conformément au correctif #27.

Le worker Android utilise désormais `CLOCK_BOOTTIME` via `rustix 1.1.6/time`,
déjà versionné dans le workspace, avec une origine locale à son démarrage.
Le temps de veille compte pour **tous** ses TTL (santé, leases et réservations),
pour conserver un domaine unique. Les constantes de TTL ne changent pas.
L'hôte conserve son horloge existante. Les propriétés de BOOTTIME sont documentées
par les [Linux man-pages](https://man7.org/linux/man-pages/man2/clock_gettime.2.html) ;
les différences entre horloges Android et Handler par
[SystemClock](https://developer.android.com/reference/android/os/SystemClock).

Le service demande l'heure du worker **avant** les appels Android ; la réception
valide cet instant sans le remplacer. À 6000 ms l'échantillon est encore frais ;
à 6001 ms il est refusé. Temps futur, retour en arrière, valeur JNI invalide,
tuple mémoire impossible, état requis absent et exception de lecture invalident
le dernier échantillon. Une lecture rejetée n'augmente pas le compteur de mesures.
La fenêtre borne l'âge de la **lecture**, pas l'âge matériel non exposé des
états mis en cache par Android.

L'invalidation conserve les allocations et les bandes protectrices antérieures,
annule seulement les fenêtres de récupération en cours et ferme l'admission.
Un intervalle entre lectures supérieur à 6 s ne compte pas comme récupération
stable. Commit refuse et libère sa réservation si la santé est devenue inconnue.
Une reprise avec un tuple complet suit les règles existantes de récupération.

Le format `HealthSample` et les messages réseau restent inchangés. Le dernier
argument JNI d'écriture est désormais l'heure retournée par
`workerHealthSampleTimeMs()` ; APK et bibliothèque doivent provenir du même build.

Pour la qualification physique, le build débogage émet aussi `HEALTH_PRECHECK`
avant chaque lecture Android. Il expose la décision ResourceGuard, l'âge et le
budget **avant** qu'une mesure de reprise puisse effacer un état périmé. La ligne
`HEALTH_SAMPLE` associe ensuite au résultat JNI le tuple Android en octets,
code thermique et pourcentage batterie, puis les bandes, l'âge et le budget
relus dans ResourceGuard. Ces champs sont des diagnostics, jamais une source
d'admission ; aucun nouveau polling, seuil ou message réseau n'en dépend.
L'horodatage natif du début de collecte et `SystemClock.elapsedRealtime()`
permettent de comparer les écarts autour d'une interruption. Le snapshot JNI
est lu champ par champ ; les lectures d'une même ligne ne sont donc pas
strictement atomiques au passage d'une frontière de péremption.

## Sources écartées

| Candidat | Décision |
| --- | --- |
| Collecteur shell quarantiné, `mLastMemoryLevel != 0` | Rejeté : ce niveau interne n'est pas `MemoryInfo.lowMemory`. Aucun réemploi ni réactivation. |
| `mIsEnabled` de Low Power Standby | Rejeté : ce service ne décrit pas Battery Saver. |
| PSS, mémoire du heap, `onTrimMemory`, PSI/cgroups ou parsing `/proc` | Non ajoutés : domaines distincts et preuve/permissions/contrat insuffisants pour remplacer les champs actuels. |
| `threshold`, `advertisedMem`, `freeMem` API 37 | Pas de nouvelle politique. `totalMem` seul sert à vérifier la cohérence du tuple existant. |
| Headroom thermique, température batterie, état interactif, Doze | Pas de substitut aux bandes thermiques ni de nouveau signal d'admission. |

## Preuves et validation reproductible

Voir [l'enregistrement des validations](../evidence/android-health-source-v0-1.json)
pour les commandes, résultats réellement obtenus et empreintes de logs.

- Tests Kotlin de conversion des API : normal, bornes, absence, UNKNOWN,
  débordement arithmétique évité et mémoire incohérente.
- Test JVM chargeant la vraie bibliothèque JNI Linux : publication/invalidation
  des fixtures Kotlin, bandes reçues, absence d'âge et budget zéro en refus.
- Tests Rust à horloge manuelle : délai de collecte, saut temporel simulant une
  veille, frontière 6 s, temps futur, ordre, mémoire, thermique, batterie,
  récupération, reserve et commit. Les tests de liaison JNI vérifient aussi les
  valeurs exactes reçues par le worker.
- Compilation de la bibliothèque Android arm64, APK et lint avec SDK/NDK/Gradle
  déjà installés. La CI exécute le nouveau test via `scripts/pipeline-android`.

Context7 et les références Android officielles ont été consultés avant le travail
manuel. Les sources AOSP ont été vérifiées au tag `android-16.0.0_r1`, compatible
avec l'API 36 du dernier appareil documenté, sans présumer de son implémentation OEM.
Ni émulateur, ni AVD, ni image système n'étaient installés. Le test JNI sur hôte
est une exécution réelle de la liaison logicielle avec des entrées simulées ;
il n'exécute pas les services Android. Les anciennes preuves A6/P2 restent
historiques et ne valident pas ce nouvel APK. Aucun benchmark n'a été relancé.

## Vérification courte restant sur appareil

Avec l'APK correspondant à la PR, sans modifier les réglages système :

1. Enregistrer version Android, build de l'application et empreinte APK.
2. Observer les lectures mémoire/batterie/Battery Saver et les valeurs reçues
   dans le worker/C08 pendant un usage ordinaire ; rechercher une variation
   mémoire naturelle, sans provoquer de pression destructive.
3. Vérifier le service thermique/HAL en lecture seule et la cohérence de la
   bande native avec celle reçue. Un NONE constant seul n'est pas une preuve.
4. Observer une courte séquence premier plan, arrière-plan et écran éteint ;
   comparer `HEALTH_PRECHECK` (avant lecture) à `HEALTH_SAMPLE` (après lecture).
   Un écart de plus de 6 s doit produire un pré-contrôle périmé/refusé et le
   retour d'un échantillon frais doit suivre les règles existantes. Une simple
   reprise des logs après l'écran éteint ne prouve pas le refus pendant l'écart.
   Toute absence connue doit rester refusée ; aucun test de chauffe ou de
   charge extrême requis.

**NEXT_ACTION : valider sur appareil la provenance thermique et la cohérence des
lectures/fraîcheur de cette source minimale avant de rouvrir le spike CPU
multicœur, puis la RAM distante.** Les simulations ferment le travail logiciel
de cette mission ; elles n'accordent aucune autorisation de reprise physique.
