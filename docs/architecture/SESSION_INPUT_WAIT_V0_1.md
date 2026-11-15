# Attente de session host réveillable — v0.1

Base : `21f664783d333024d9b336a7423dfb7e197a9318`, fusion de #33.
Branche : `spike/session-input-wait-v0-1`. A/B physique réalisé, six mesures exactes ; aucun merge de ce chantier.

## Audit et choix

`pb-runtime-secure/src/runtime.rs` possède `SESSION_POLL=20 ms` et la boucle
authentifiée. Le lecteur fait un peek nonbloquant du préfixe : WouldBlock ou
un seul octet => aucune frame ; un préfixe complet bascule le lecteur existant
en lecture bloquante bornée à 5 s. Le framing et ce délai ne changent pas.
La boucle conserve les contrôles annulation, heartbeat 2 s, session idle 10 s
et requête active 35 s. Les requêtes mpsc entrantes restent observées au prochain
tour, au plus après le quantum existant ; pas de nouvelle primitive de queue.

`pb-host/src/auto_use.rs` possède le thread session, cancel_session et join.
La Condvar de `initiator.rs` réveille les appelants qui attendent l'état de
session ; les résultats utilisent un canal avec timeout. Elle n'est pas un
mécanisme de réveil du socket. Android utilise le même runtime comme responder
via core-jni ; ses sleeps de session et des handlers RemoteBuffer/compute
restent inchangés, notamment pour observer la perte du transport durant un
handler. L'APK installé reste exactement celui de #33.

Trois options examinées :

| Option | Décision |
| --- | --- |
| peek bloquant avec read_timeout 20 ms | Possible, mais exige des mutations/restaurations du mode et du timeout socket, avec arrondi noyau. |
| rustix poll/ppoll avec délai explicite 20 ms | Retenue : dépendance déjà installée, seule feature event activée, aucun changement de mode socket, aucun nouveau thread. |
| Multiplexage Condvar/eventfd/queue, ou polling 1 ms | Écarté : surface supplémentaire ou fréquence accrue sans nécessité pour attendre les réponses réseau. |

API vérifiée par Context7 et source locale rustix **1.1.6** :
[poll](https://docs.rs/rustix/1.1.6/rustix/event/fn.poll.html).
Aucune version de dépendance ni lockfile changé. Un délai est toujours fourni.

La branche initiateur Linux utilise poll ; le responder Android garde sleep.
Données disponibles : aucun délai ajouté. Socket vide : réveil par données,
EOF/erreur, ou expiration du même quantum de 20 ms. NVAL et erreur terminale
échouent SessionLost ; HUP/ERR réveillent le lecteur existant. EINTR termine
le quantum avec un sleep sur le temps restant, sans boucle de retry ni spin.

Un préfixe d'un octet est déjà lisible et provoquerait un spin avec poll naïf.
Ce cas conserve le sleep 20 ms ; un poll nonbloquant RDHUP/HUP/ERR/NVAL détecte
cependant une fermeture derrière le préfixe tronqué et refuse immédiatement.
Le parser, Noise/PBMUX, leases, annulation/cleanup et ResourceGuard ne changent pas.
Les garanties temporelles restent des bornes logiques, avec retard scheduler possible.

## Instrumentation et protocole de preuve

Trois compteurs diagnostiques sont ajoutés à la fin de la liste existante :
SessionReadableWake, SessionTimeoutWake, SessionPartialWait. Ils sont purement
numériques et soumis au même opt-in. SessionPoll mesure toujours la durée
complète de l'attente ; les compteurs de réveil ne sont pas ajoutés aux durées.
Le collecteur opérateur accepte explicitement les 59 métriques de l'APK gelé
et les 62 de l'hôte nouveau, en conservant l'ordre du préfixe inchangé.
Les anciens diagnostics sont vérifiés contre leur commit source enregistré,
et non contre les nouvelles sources optimisées ; la CI récupère cet historique.

Banc réutilisé de #33 : deux warmups puis trois paires baseline/nouveau,
binaires host/CLI release normaux, même APK/entrée 64 MiB/USB/CPU2/digest.
Préflight inchangé : Health NOMINAL, HAL Ready sans override, Saver OFF,
mémoire ≥896 MiB pendant ≥10 s, budget suffisant. Aucun réglage de protection.
Avant chaque run, 5 s sans workload mesurent CPU idle host/Android. CPU Android
est lu via run-as /proc/pid/stat ; CLK_TCK vérifié à 100, fenêtre ADB plus large
que l'intervalle CLI et résolution de 10 ms documentées. Aucune soustraction
entre horloges Linux et Android.

Les tests ajoutés couvrent données déjà présentes, arrivée différée, timeout,
préfixe partiel sans spin, fermeture et shutdown, FIN derrière préfixe tronqué,
annulation réelle d'un initiateur authentifié. Les suites existantes couvrent
liveness, leases, réponses, crypto/framing invalides, timeout et cleanup.

## Résultat physique

Deux warmups exclus, puis trois paires alternées baseline/nouveau, toutes exactes
REMOTE_SUCCESS. Même APK `org.phoneboost.app.multicore`, SHA-256
`7ece5cb887986d5408492915489432065c19c352efae19b0c228fef104bf0ce4`.
Samsung SM-A155F Android16/API36. Build host/CLI release standard Rust1.98,
mêmes features experiment/diagnostic et lockfile ; seules les features rustix
event s'ajoutent, sans nouvelle version/crate. Empreintes des binaires et des
sources mesurées dans [les preuves](../evidence/session-input-wait-v0-1.json).

| Paire | Baseline ms | Nouveau ms | Gain |
| --- | ---: | ---: | ---: |
| 1 | 3415,641 | 3391,031 | 1,0073× |
| 2 | 3398,479 | 3293,678 | 1,0318× |
| 3 | 3464,192 | 3226,650 | 1,0736× |

Gain apparié médian **1,0318×**, 3/3 paires favorables. Ratio des médianes
indépendantes **1,0370×**, soit **121,963 ms / 3,5717 %** de temps total en moins.
Le gain observé est modeste et son amplitude variable (0,72–6,86 % par paire) :
ce petit échantillon ne garantit pas un gain universel ou statistiquement établi.
Aucune optimisation supplémentaire n'a été lancée pour augmenter le chiffre.

Médianes indépendantes ; ratio baseline/nouveau pour durées/compteurs,
nouveau/baseline pour débits. Les lignes ne sont pas additives.

| métrique | baseline | nouveau | ratio |
| --- | ---: | ---: | ---: |

| End-to-end ms | 3415.641 | 3293.678 | 1.0370× |
| Boucle PUT ms | 2901.636 | 2760.225 | 1.0512× |
| SessionPoll host ms | 3162.253 | 3072.854 | 1.0291× |
| SessionPoll Android ms | 1571.883 | 1516.202 | 1.0367× |
| Handlers poll Android ms | 471.357 | 471.219 | 1.0003× |
| Encrypt host ms | 89.581 | 83.763 | 1.0695× |
| Decrypt Android ms | 744.794 | 736.031 | 1.0119× |
| WireWrite ms | 64.159 | 27.563 | 2.3277× |
| Débit PUT MiB/s | 22.057 | 23.187 | 1.0512× |
| Débit end-to-end MiB/s | 18.737 | 19.431 | 1.0370× |
| CPU host ms | 300.000 | 270.000 | 1.1111× |
| CPU processus Android ms (fenêtre ADB) | 1130.000 | 1140.000 | 0.9912× |
| CPU idle host % d’un CPU | 0.400 | 0.400 | 1.0000× |
| CPU idle Android % d’un CPU | 3.000 | 2.400 | 1.2500× |
| Nombre waits host | 157.000 | 166.000 | 0.9458× |
| Réveils lisibilité host | 0.000 | 27.000 | — |
| Réveils timer host | 157.000 | 140.000 | 1.1214× |
| Attentes préfixe partiel | 0.000 | 0.000 | — |
| ALLOC ms | 60.747 | 64.769 | 0.9379× |
| STAT ms | 60.999 | 63.465 | 0.9611× |
| FREE + deux RELEASE, total ms | 60.504 | 63.638 | 0.9507× |
| compute.submit ms | 100.898 | 104.934 | 0.9615× |
| Compute natif ms | 49.820 | 50.217 | 0.9921× |

Les petites requêtes ALLOC, STAT, cleanup et compute.submit ne s'améliorent pas
(quelques ms de plus en médiane). C'est une limite réelle du patch, pas une
régression de réponse/protocole ; aucune promesse de gain sur chaque opération.
RELEASE n'est **pas isolé** : le compteur existant RemoteCleanup enveloppe FREE
puis les deux RELEASE (scratch/classe1) ; aucune durée d'un RELEASE seul n'est
inventée. Le total est conservé pour comparer exactement le même chemin.

CPU idle host : **0,4 % d'un CPU** dans les deux groupes, mesuré pendant 5 s
avant chaque run (10–20 ms CPU, résolution ticks 10 ms). Aucun remplacement de
l'attente par un busy-loop. CPU host actif 300→270 ms ; CPU Android 1130→1140 ms,
avec résolution 10 ms et fenêtre ADB plus large que CLI. Pas de stress, d'affinité,
de nouveau workload, ni de modification des réglages host/Android.

Health est NOMINAL avant/pendant/après les jobs, HAL Ready sans override,
budget128 MiB, availMem minimum **1028,6–1078,0 MiB** selon le run,
âge au compute≤1682 ms, lowMemory=false, Battery Saver OFF, aucun refus.
Batterie/charge et températures brutes restent archivées ; aucune valeur synthétique.

### Concordance et fenêtres

Toujours 17 PUT, 16×4194240+1024 octets ; mêmes digest/réponses/frames utiles.
Hors Control/Metrics, exactement **1113 frames TX / 25 RX**, avec concordance
bidirectionnelle des octets utiles. La capture Android entoure la fenêtre host
avec **147–215 ms** de marge maximale, ce qui inclut sur certains runs jusqu'à
deux frames Control/Metrics supplémentaires. Le JSON conserve tous les compteurs
bruts et leurs écarts. Le validateur ne normalise ni ne remplace les données :
il vérifie les payloads exactement et borne les écarts Control séparément,
ainsi que le framing chiffré local (préfixe2 + tag16 octets par frame).
Une perte de frame utile, un écart non expliqué ou plus de deux contrôles sont
refusés. Les anciens diagnostics gardent leur validation stricte par défaut.

### Goulot restant et décision

Le nouveau réveil est effectif : **26–28 réveils lisibilité** par run,
contre des réveils uniquement timer en baseline. Mais il reste **139–140
expirations de 20 ms**, soit près de **2,8 s** : `SessionPoll` host reste à
**3,073 s**, largement dominant. Android session poll vaut **1,516 s**, les
handlers **471 ms**, Decrypt **736 ms**. Ces durées sont imbriquées et ne
s'additionnent pas aux 3,294 s end-to-end. L'attente host n'est pas un RTT USB.

Le code montre que la queue de requêtes locales ne réveille pas le poll socket ;
Android conserve ses propres sleeps. Leur contribution causale respective aux
3,07 s restantes n'est **pas encore isolée**. Le patch démontre seulement un
gain end-to-end modeste, un réveil réseau correct et l'absence de spin. Il ne
justifie ni nouvelle crypto, ni changement de chunks, ni conclusion de saturation
USB/Ethernet. Le prochain chantier doit instrumenter l'attente restante, pas
attribuer arbitrairement toute sa durée à Android Decrypt ou au transport.

## Validation et restauration

Suite release initiale : 303 tests +4 doctests, dont les nouveaux tests I/O.
Après ajout du cas FIN/préfixe et annulation authentifiée : suite runtime finale
**31 tests +4 doctests PASS**. Clippy passe selon la politique existante (warnings
préexistants hors patch). OFF/ON et baseline/nouveau : sorties CLI, erreurs,
digest, codes de retour et fallback identiques. Tests de preuves rejettent
pertes de données, faux succès, Health stale, binaire incorrect et CPU idle excessif.
La CI réexécute les suites et la validation des preuves ; elle ne remplace pas
la preuve physique.

Restauration vérifiée 2026-11-15 14:17:05 UTC : **Battery Saver ON, profil1,
collecte OFF, APK expérimental arrêté, daemon/forward isolés arrêtés, auto-use OFF**.
APK produit et APK expérimental inchangés. Aucun worktree/branche historique
supprimé. Le merge de #33 est un merge commit à deux parents, sans squash/rebase.

**NEXT_ACTION : Décomposer les 3,07 s d’attente résiduelle des PUT entre file host et polling Android avant toute seconde optimisation.**
