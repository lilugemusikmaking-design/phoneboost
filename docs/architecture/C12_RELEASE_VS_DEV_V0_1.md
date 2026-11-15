# C12 — host release standard contre dev, preuve physique v0.1

Le **build release standard supprime le goulot Noise observé en dev**. Le
contrôle principal, deux warmups puis trois paires alternées dev/release,
donne **34,855 s → 3,586 s** en médiane : **9,720×**, soit **89,712 % de temps
en moins**. Les trois paires sont favorables (9,720× / 10,734× / 9,060×).
Noise host passe de **32,490 s à 90,160 ms**, soit 360,360× moins de temps.
Aucune logique PhoneBoost, dépendance, chunk, protocole, timeout, ResourceGuard
ou APK n'a changé : seul le profil Cargo des binaires host/CLI diffère.

Le nouveau poste dominant mesuré est **SessionPoll host : 3 324,546 ms** en
médiane, principalement pendant les PUT séquentiels. C'est une attente avec
sommeils inconditionnels de 20 ms, **pas un RTT USB**. Elle inclut le temps
pendant lequel Android reçoit, déchiffre, traite et répond ; sa part évitable
n'est pas encore isolée. Aucune optimisation n'est implémentée ici.

[Preuves numériques et états host](../evidence/c12-release-vs-dev-v0-1.json) —
[diagnostic dev historique](C12_END_TO_END_TIMING_RESULTS_V0_1.md) —
[preuves dev inchangées](../evidence/c12-end-to-end-timing-v0-1.json).

## Build et environnement réellement appliqués

Source compilée : `31ecca48a347a95c1650cf08822e82cf82889e88`, PR #33.
Le commit de ce rapport ajoute uniquement preuves, outils de validation, CI
et état documentaire ; aucune modification des sources Rust/Android depuis
ce SHA. Leurs empreintes sont identiques à la campagne dev initiale.

```sh
PHONEBOOST_TIMING_DIAGNOSTIC=1 \
CARGO_TARGET_DIR=/home/mimir/phoneboost-dev-pipeline-v1/target \
CARGO_BUILD_JOBS=1 \
cargo build --offline --locked --release -v -p pb-host -p pb-cli \
  --features pb-host/experimental-multicore,pb-host/diagnostic-timing,pb-cli/diagnostic-timing
```

Rust **1.98.0 (88d9e12ae 2026-08-18)**, LLVM 22.1.8,
`x86_64-unknown-linux-gnu`. Le [profil Cargo release standard](https://doc.rust-lang.org/cargo/reference/profiles.html)
utilise opt-level 3, sans debuginfo, debug assertions/overflow checks OFF,
panic unwind, incremental OFF, 16 codegen units et LTO par défaut.
Les commandes rustc enregistrées montrent `-C opt-level=3`,
`-C embed-bitcode=no`, `-C strip=debuginfo`, générés par Cargo.
Aucun profil intermédiaire ni profil release spécial.

L'audit de l'environnement enregistre explicitement :

- `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`, `RUSTC_WRAPPER`,
  `RUSTC_WORKSPACE_WRAPPER`, `CARGO_BUILD_RUSTFLAGS`,
  `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS`, `CARGO_INCREMENTAL`
  et `CARGO_BUILD_INCREMENTAL` : **non définis**.
- `CARGO_PROFILE_*` : aucune variable ; aucun `[profile.*]` dans le workspace.
- Aucun `.cargo/config` ni `.cargo/config.toml` applicable dans le worktree,
  ses parents `/tmp` et `/`, ou `/home/mimir/.cargo` ; `CARGO_HOME` non défini.
- Aucun `target-cpu=native`, LTO forcée ou wrapper. `CARGO_BUILD_JOBS=1`
  limite seulement la compilation T440. Collecte runtime activée par
  `PHONEBOOST_TIMING_DIAGNOSTIC=1` dans les deux profils.

Features exactes communes : `pb-host/experimental-multicore`,
`pb-host/diagnostic-timing`, `pb-cli/diagnostic-timing`.
L'APK Android préexistant conserve `diagnostic-timing`, son natif release et
le profil CPU 2 ; sa réservation scratch reste donc identique.

SHA-256 des exécutables gelés et vérifiés avant la campagne :

| Profil / binaire | SHA-256 |
| --- | --- |
| dev phoneboostd | `34b0f3a51cabd9e8a51feacc802c40be9e3dead57d20a1e47dd50a7e627f9e22` |
| dev phoneboostctl | `37c076cf50ff9c8e549d82af549dd7dd4dabdb2bbae4a78792122eee2a77297b` |
| release phoneboostd | `4ed9da9d5b92aeda0e1eaea3024f9328027cd5c01e5b6601f8164de96d2aa271` |
| release phoneboostctl | `4b23b96f8bf9d1f862f1497aae2d8f2748e737c59e8fc81e8d033721edcb4b74` |

`Cargo.lock` inchangé : `788dd17646b9f725ca7e382e7d04819155f159038f0122c9ea86438de045e605`.
Cargo workspace resolver **2** ; `snow` **0.9.6**, même
**DefaultResolver RustCrypto** dans les deux profils. Les fingerprints dev
et release confirment exactement `aes-gcm`, `blake2`, `chacha20poly1305`,
`curve25519-dalek`, `default`, `default-resolver`, `sha2`, et `rustflags=[]`.
Aucun resolver Ring/libsodium ajouté. Vérification directe de
`snow-0.9.6/src/builder.rs:61-69` et `resolvers/default.rs` ; les builders
PhoneBoost et les suites `Noise_XX_25519_ChaChaPoly_SHA256` /
`Noise_IK_25519_ChaChaPoly_SHA256` restent identiques. PBMUX, framing, chunks,
contrôles d'intégrité et comportement réseau restent inchangés.

## Contrôle ABAB et conditions physiques

Hôte **Intel Core i5-4300U @ 1.90 GHz**, quatre CPU logiques, alimentation
secteur confirmée (`AC online=1`), batteries 99 %/100 %. Driver
`intel_cpufreq`, governor **performance**, Intel pstate **passive**,
`no_turbo=0`, bornes 800–2 900 MHz, min/max perf 27/100. Aucun de ces réglages
n'a été écrit. Les snapshots avant/après chaque mesure enregistrent :
fréquences **1 887–2 672 MHz**, loadavg 1 minute **3,37–5,12**, température
CPU host **58–61 °C**. Ce sont des lectures instantanées, pas des moyennes
pendant le job. L'hôte n'est pas isolé de toute activité de bureau ; aucun
build ni autre workload lourd de cette mission ne tourne pendant la campagne.
Le coût de ces lectures, **6,2–16,7 ms**, est hors de l'intervalle CLI mesuré
et hors de la fenêtre des compteurs Android.

Séquence réelle : **warmup dev → warmup release → D1/R1 → D2/R2 → D3/R3**.
Chaque changement de profil arrête uniquement le daemon diagnostique,
réutilise la même identité appairée et le même forward ADB, puis attend READY.
Pas de nouvelle négociation de confiance ni d'installation d'APK. Les deux
warmups sont conservés mais exclus des médianes. Un contrôle de 40 octets
supplémentaire apparaît dans la fenêtre Android, plus large, du warmup dev :
écart explicitement enregistré, concordance payload exacte. **Les six runs
mesurés ont une concordance totale frames/octets exacte**, sans cette marge.

Samsung **SM-A155F**, Android 16/API 36, build
`samsung/a15nseea/a15:16/BP4A.251205.006/A155FXXSBEZG1:user/release-keys`.
Même APK `org.phoneboost.app.multicore`, sans reconstruction/installation,
SHA-256 `7ece5cb887986d5408492915489432065c19c352efae19b0c228fef104bf0ce4`.
Même entrée **67 108 864 octets**, même C12/tmpfs, même PBMUX/Noise via forward
ADB USB, même profil CPU 2, aucune affinité, aucun Perfetto.
Digest BLAKE3 exact : `7b2def9fc609feb94905bd0cdb0772743544d6564b8d73cf2bba66c4759116a4`.

Avant chaque run : USB stable, Battery Saver OFF, HAL Ready sans override,
Health fraîche/NOMINAL, mémoire ≥896 MiB pendant ≥10 s, budget 128 MiB.
Durant les six mesures : mémoire min **919,2–1 052,3 MiB**, thermique 0,
lowMemory=false, budget 128 MiB, aucun refus. Âge Health au compute ≤1 445 ms.
**Six REMOTE_SUCCESS exacts**, stdout identique aux runs dev historiques ;
les timings restent uniquement sur stderr. Aucun fallback compté comme succès.

| Paire | dev total ms | release total ms | gain apparié | release Noise ms | release poll/wait ms |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 34855,015 | 3585,781 | 9,720× | 90,160 | 3358,087 |
| 2 | 37618,010 | 3504,585 | 10,734× | 86,572 | 3174,675 |
| 3 | 33163,513 | 3660,396 | 9,060× | 125,221 | 3324,546 |

## Comparaison principale

Médianes sur les trois runs de chaque profil ; **ratio = dev/release pour les
durées, release/dev pour les débits**. Ces lignes ne forment pas une partition
additive. Les gains appariés sont calculés séparément ci-dessus.

| mesure | dev médian | release médian | ratio |
| --- | ---: | ---: | ---: |
| End-to-end (ms) | 34855.015194 | 3585.780820 | 9.720342× |
| Noise host (ms) | 32490.099797 | 90.160095 | 360.360088× |
| Noise (ms/frame) | 28.854440 | 0.080861 | 356.839697× |
| Débit Noise (MiB/s) | 1.971200 | 710.336289 | 360.357234× |
| Total hors Encrypt (ms) | 2364.915397 | 3495.620725 | 0.676537× |
| Boucle PUT (ms) | 34299.907654 | 2939.627953 | 11.668112× |
| Débit PUT utile (MiB/s) | 1.865894 | 21.771463 | 11.668112× |
| Session poll/wait host (ms) | 1875.267564 | 3324.546340 | 0.564067× |
| Write socket host (ms) | 180.940292 | 64.698982 | 2.796648× |
| Android Decrypt (ms) | 1648.204469 | 738.288237 | 2.232467× |
| Android session poll (ms) | 31661.582911 | 1734.167917 | 18.257507× |
| Android handlers poll (ms) | 489.326459 | 489.529076 | 0.999586× |
| Compute natif (ms) | 50.486539 | 50.962461 | 0.990661× |
| Résidu chemin critique (ms) | 17.792768 | 18.735672 | 0.949673× |
| Débit end-to-end utile (MiB/s) | 1.836178 | 17.848274 | 9.720342× |

Noise utilise les octets réellement chiffrés (`Encrypt.bytes`),
PBMUX/contrôle inclus ; PUT utile et end-to-end utilisent exactement 64 MiB.
Le débit Noise ne décrit pas le câble. Les **parts par run puis médianées**
sont **2,514 % Noise** (2,470–3,421 %) et **90,825 % SessionPoll**
(90,586–93,650 %). Le ratio des médianes wait/total, différent, est 92,715 %.
Le résidu médian est **18,736 ms** ; celui du run médian pair01-release est
25,266 ms. Toutes les partitions CLI/host sont cohérentes, résidu <1 %.

Les trois mesures release groupées, réalisées avant la demande ABAB, restent
intégralement dans `grouped_preliminary` : **3,563 s** médian, **101,712 ms**
Noise, **22,124 MiB/s** PUT. Face aux trois dev historiques (26,502 s,
24,414 s Noise), elles donnent **7,439×**, soit **86,557 %** de réduction.
Elles ne sont pas mélangées aux médianes ABAB. Les dev actuels sont plus lents
que les historiques ; les conditions host enregistrées et la variabilité
empêchent d'attribuer cet écart à une cause précise. Les deux campagnes
concordent sur le verdict : Noise s'effondre et release termine autour de 3,5 s.
Aucun gain universel précisément égal à 9,720× n'est revendiqué.

## Nouveau poste dominant et limites de l'attribution

Cas **A**, puis **C** : le build dev constituait le problème principal ;
la boucle d'attente session devient dominante. Le CPU du processus host
passe de **30,20–34,08 s à 300–350 ms**. Android compute reste autour de 51 ms.
Les **17 PUT** restent 16 × 4 194 240 + 1 × 1 024 octets. Leur boucle médiane
est **2,940 s**, soit **21,771 MiB/s** utiles ; le débit end-to-end est
**17,848 MiB/s**. Aucun changement de transport ou de taille de bloc.

`SESSION_POLL = 20 ms` reste transmis à `std::thread::sleep` :
**92/93/94 appels dev** contre **167/158/165 release**, soit environ 20,1 ms
par appel. Le coût host **hors Encrypt augmente** de 2,365 à 3,496 s.
Une émission host plus rapide change la disponibilité des réponses et le
nombre de passages dans cette attente. **SessionPoll n'est pas une mesure
de capacité réseau ni un RTT USB pur** ; sa totalité n'est pas un gain acquis
par suppression des sleeps.

Côté Android, Decrypt médian est **738,288 ms**, polling des handlers
buffer+compute **489,529 ms**, idle/poll session **1 734,168 ms**, lecture socket
**450–610 ms**. Ces durées sont prises dans le domaine Android et se chevauchent
avec l'attente host ; on ne les additionne pas au total Linux. La marge de
capture Android est bornée à **186–208 ms** sur les trois release (ADB autour
du job), et son idle peut inclure cette marge. Les différences Android malgré
un APK identique peuvent venir de la cadence, du scheduling et des fréquences ;
ces causes ne sont pas séparément démontrées.

`WireWrite` host reste faible (**64,699 ms** médian, **59–65 ms**), sans preuve
de contre-pression dominante. Le buffering socket/ADB interdit d'en déduire
une capacité maximale USB/Ethernet. La prochaine intervention doit cibler
la latence de réaction de la session aux réponses PUT, avec mesure A/B de sa
part réellement évitable ; aucune optimisation crypto n'est justifiée ici.

Frames host TX/RX dev : **1126/56, 1128/59, 1126/55** ; release :
**1115/29** chaque fois. Hors Control/Metrics, **1113 TX / 25 RX** dans chaque
run des deux profils. Concordance totale avec Android vérifiée.
Toujours **0 compute.status**, **0 acquisition de bail synchrone** ;
renouvellements dev 2/1/1 et release 0/0/0. Polls import READY : dev 4/4/3,
release 4/3/5. Le validateur conserve l'invariant réel : un poll terminal,
un sleep par RECEIVING ; il ne fige pas le nombre de polls dépendant du timing.

## Validations, preuves et restauration

Avant téléphone : **300 tests + 4 doctests release** passent sur pb-types,
pb-cli, pb-host et pb-runtime-secure, collecte activée. Tests C12 par vrais
processus PASS. Fixtures OFF/ON et dev/release : stdout, digest, erreurs,
codes de retour et fallback identiques. Les tests réseau existants couvrent
les échanges authentifiés et le résultat distant 64 MiB.

Microbenchmark de 1 000 000 spans : OFF 1,454 ms, ON 145,809 ms,
**144,355 ns additionnels/span**. Il mesure le compteur local, pas un A/B
physique complet ; ~7 000 enregistrements host par run donnent un ordre de
grandeur de 1 ms. L'instrumentation ne peut expliquer le changement de dizaines
de secondes. Les snapshots host sont hors intervalle mesuré.

```sh
python3 scripts/test_compare_c12_build_timing.py
python3 scripts/compare_c12_build_timing.py \
  --dev docs/evidence/c12-end-to-end-timing-v0-1.json \
  --release docs/evidence/c12-release-vs-dev-v0-1.json
```

Ces validations recalculent les médianes/paires et rejettent fallback,
stale, altération de frames, d'APK, d'entrée, de lockfile, de binaire,
d'ordre ABAB, confusion warmup/mesure et gain falsifié. Elles vérifient aussi
les preuves groupées conservées et les empreintes des sources.
La CI ajoute un job release diagnostique séparé, avec les mêmes tests et
l'équivalence dev/release. Cette CI vérifie des preuves enregistrées ; elle
ne prétend pas fournir une preuve physique.

Les bruts, binaires, scripts opérateur et logs sont archivés en local avec
accès privé ; seules les données numériques et empreintes autorisées sont
publiées. Aucune clé, identité d'appairage ou donnée utilisateur dans les preuves.

Restauration finale vérifiée **2026-11-15 13:34:08 UTC** : Battery Saver ON,
profil expérimental 1, collecte OFF, APK expérimental arrêté, auto-use isolé OFF,
daemon et forward de cette campagne arrêtés. Empreinte APK produit inchangée.
Aucune branche/worktree historique supprimée. PR #33 reste ouverte.

**NEXT_ACTION : Réduire la latence du polling de session à 20 ms sur le chemin PUT en release, avec mesure A/B et protocole inchangé.**
