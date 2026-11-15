# Diagnostic des ~28 s end-to-end — périmètre de mesure v0.1

Base : `master` après fusion de la PR #32, `524d0065f5609919c61e37c242771ee3bf61eb9b`.
Branche : `spike/c12-end-to-end-timing-v0-1`.
État au checkpoint `81df7e2` : périmètre défini. Plan désormais exécuté ;
[réalisation et résultats physiques](C12_END_TO_END_TIMING_RESULTS_V0_1.md).
Les bornes ci-dessous restent le plan historique validé, sans redéfinition.

La [campagne CPU physique](ANDROID_MULTICORE_SPIKE_V0_2.md) mesure environ 28 s
entre l'appel CLI et son résultat exact, alors que le compute natif dure des
dizaines de millisecondes. Le goulot des 28 s reste inconnu. Cette étude mesure
le chemin C12 existant avec la même entrée de 64 MiB, le même digest attendu,
le même transport et l'APK expérimental isolé. Elle ne change ni la décision
ResourceGuard, ni les délais, le protocole, le format des réponses, les profils
CPU ou le chemin produit.

## Bornes de mesure prévues

| Niveau | Bornes à observer | Ce que la durée signifie |
| --- | --- | --- |
| CLI, `pb-cli/src/import.rs` | début/fin de la commande, connexion locale, `imports.create`, attente jusqu'au premier `READY`, `compute.submit_import`, validation du résultat | partition du temps visible par l'appelant ; nombre et durée des polls inclus dans l'attente READY |
| Import C12, `pb-host/src/import_store.rs` | acceptation du descripteur, démarrage du receiver, lecture, écriture, `flush`/`fsync`, renommage vers READY | copie et persistance locales ; temps du worker asynchrone, qui chevauche l'attente CLI et ne s'additionne pas à elle |
| Traitement local, `pb-host/src/local_api.rs` | événement par méthode et traitement de `compute.submit_import` | coût de validation, consommation du handle et passage à l'auto-use ; réutiliser `LocalApiRequestEvent` déjà présent comme point d'observation |
| Hôte distant, `pb-host/src/auto_use.rs` | attente du verrou d'opération, décision de placement, réserve/commit, `ALLOC`, boucle `PUT`, `STAT=READY`, réserve scratch, `compute.submit`/`status`, libération | décomposer le temps du submit, notamment lecture des blocs source et aller-retour de chaque `PUT` ; conserver nombre de requêtes et octets |
| PBMUX/transport, `SessionAccess` et client sécurisé | temps d'aller-retour par type de requête du job, temps d'encodage/écriture/attente/lecture/décodage lorsque disponible | distinguer attente transport de coût local ; exclure le trafic de heartbeat et de renouvellement hors du job |
| Android, JNI/`WorkerCore` | attente du verrou JNI, traitement `RemoteBuffer` (`ALLOC`, `PUT`, `STAT`), admission, compute déjà tracé, réponse | coût sur l'appareil, mesuré séparément du temps d'aller-retour hôte ; vérifier où se placent les copies du `PUT` |

Le bail et la readiness de la session sont normalement maintenus par le
contrôleur en arrière-plan avant `compute.submit_import`. Leur état et les
événements de renouvellement seront enregistrés avec le job, mais leur durée ne
sera imputée à ses 28 s que si un intervalle synchrone est réellement observé.
Les réservations de buffer et de scratch, elles, sont dans le parcours du job.

Chaque processus rapporte ses **durées monotones locales**. Les horloges de
l'hôte et d'Android ne sont pas soustraites directement l'une de l'autre. La
vue CLI donne la partition externe ; les mesures hôte et Android sont des
sous-étapes, parfois emboîtées, et ne doivent pas être additionnées deux fois.
Un résidu non attribué reste explicite. Les temps du hash natif restent ceux
produits par PhoneBoost ; Perfetto sert uniquement si une frontière reste
ambiguë et la capture ne doit pas fausser l'admission mémoire.

## Sortie et validation prévues

L'instrumentation sera activable seulement pour le diagnostic, avec des
compteurs bornés et un résumé privé par run : durées, nombre d'appels,
octets, résultat distant exact, état Health/ResourceGuard, version d'APK et
conditions USB. Aucun contenu, handle, identifiant de session ou clé n'entre
dans les journaux publiés. Les erreurs et refus restent visibles ; aucun
fallback local ne peut être compté comme résultat distant.

Valider d'abord sur fixtures les bornes, les erreurs et la somme de la partition
CLI, puis vérifier que l'instrumentation n'altère pas les réponses ni les
délais. Sur appareil réel : quelques runs reproductibles à entrée identique,
APK exact, Health admissible et digest exact ; conserver les résultats bruts et
comparer les durées par phase et leur variabilité avant de nommer un goulot.
Sans appareil, le verdict reste **NON MESURÉ**. Aucun seuil, politique, affinité,
workload ou optimisation n'est modifié dans ce checkpoint.

**NEXT_ACTION : Compiler l’hôte diagnostique en release et remesurer le même run 64 MiB pour réduire et vérifier le coût dominant du chiffrement Noise.**
