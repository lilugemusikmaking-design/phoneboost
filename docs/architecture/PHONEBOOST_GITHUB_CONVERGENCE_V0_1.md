# PhoneBoost — convergence GitHub après P14

14 novembre 2026. Ce checkpoint relie l'historique local approuvé au `master`
après intégration de #30, #27 et #28 ; la PR #29 porte cet état documentaire.
Il ne modifie aucun contrat ni comportement produit.

## Baseline et périmètre publié

La baseline distante **historique**, avant ces PR, était `origin/master` à
`1312cb1e7ddc40e71a9512d85663264eb63b30c6` (PR24 déjà fusionnée).
Après #30, #27 et #28, le parent `master` de ce checkpoint est
`d29f78825c0444c11042e2b0279674f9a693a39f`. Le SHA final contenant la PR
documentaire #29 se résout avec `git log -1 --format=%H -- .pipeline/STATE.json`
sur `master` ; il ne peut pas être écrit dans son propre contenu.
Le dépôt est public. La référence historique locale, conservée sans réécriture,
est `spike/guarded-cpu-allocation-v0-1` à
`9daae7fb496da2f903212d7c811357901f4b2aee`, parent technique
`f5db094128f55ff79fc8038ef9d1a233ca4139cd`, soit +31/-0 sur cette
**ancienne** baseline.
Ces SHA historiques ne sont pas publiés par cette consolidation.

Ordre d'intégration vérifié :

| PR | Commit intégré dans `master` | Contenu et validation |
|---|---|---|
| #30 | `abef20b08b90fe3bbe57e4953dcc8770e9b9fa3a` | `rustix` 1.1.6, payload Android CLI exact et oracle de test JNI ; Project pipeline et CodeQL verts |
| #27 | `02889add9cb45fa9cd86510cbe6c8b6a89c2a38f` | Horloge JNI, polling, renouvellement des baux et tests ; diff produit inchangé après rebase, Project pipeline et CodeQL verts |
| #28 | `d29f78825c0444c11042e2b0279674f9a693a39f` | Aide USB conditionnelle et tests ; diff frontend inchangé après rebase, Project pipeline et CodeQL verts |
| #29 | Résolu par le commit contenant ce fichier | État documentaire final ; les checks de cette PR et du `master` final sont à lire sur GitHub |

Les HEAD de branches, résultats CI vérifiés et résultats logiciels historiques
sont consignés dans le [registre de consolidation](../evidence/post-p14-consolidation-v0-1.json).
Concernant les dépendances, #30 n'actualise que `rustix`, affectée par un avis de sécurité ;
#27 et #28 n'ajoutent aucune dépendance. Aucune licence n'est modifiée. Aucun
des 31 commits historiques n'est repris comme ancêtre des branches publiées.

## Correctifs produit extraits

| Correctif | Source locale | Extraction et limite |
|---|---|---|
| Horloge santé JNI | `4720fc7` | Lecture de santé dans l'horloge du worker ; validation de l'argument JNI conservée. Tests de fraîcheur et hystérésis. Ne qualifie pas le collecteur du spike. |
| Polling | `1b3f0b5` | Attente seulement sans trame complète ; contrôles de liveness, annulation et heartbeat conservés. Tests socket vide/préfixe partiel et trames en file. |
| Baux courts | `e24e979`, `39d097b` | Renouvellement avancé selon le TTL avant readiness/publication et en session. Échec : retrait d'autorité et fermeture. Tests TTL 500 ms, cadence et OFF avec Renew déjà admis. |
| Aide USB | `beb578e` | Affichée seulement pour préférence ON confirmée, statut LIVE frais et `NO_DEVICE`. Conseil sans action automatique ni diagnostic affirmé. `NO_HINT` avec IP présente demeure ouvert ; aucune acceptation visuelle nouvelle. |

Les correctifs n'ont aucune dépendance expérimentale indispensable. Les traces
de phases et événements transport sont exclues, y compris celles mêlées au
correctif de bail. La restauration des écritures préfixe/corps de `39d097b` ne
requiert aucun patch : la baseline distante possède déjà ces écritures séparées.
Le coalescing intermédiaire `e6bc314` n'est pas extrait.

## Preuves : logiciel des extractions et matériel historique

Le registre associé distingue les commandes exécutées sur les arbres reconstruits
avant intégration, les CI des PR fusionnées et les preuves physiques enregistrées
auparavant. Aucun téléphone n'a été utilisé pour cette consolidation. Aucun APK
ni binaire reconstruit n'a été installé chez l'utilisateur. La suppression de
l'instrumentation et la reconstruction depuis
master produisent des arbres différents : les anciennes mesures ne certifient
ni les nouveaux binaires ni leurs performances.

| Preuve historique | Résultat enregistré | Limite |
|---|---|---|
| P14 installé, 15 octobre | 1 MiB distant, OFF local, ON distant ; 64 MiB distant ; 128 MiB refusé et fallback exact | Validation sur l'arbre historique |
| Polling USB, 25 octobre | 3/3 résultats exacts ; médiane distante 6,069 s contre 25,049 s avant ; local 0,405 s | Amélioration observée du chemin distant, aucun gain net qualifié |
| Baux USB, 25 octobre | Trois jobs exacts à 2,352 / 27,347 / 67,348 s de session ; zéro événement terminal | Continuité normale au-delà de 60 s ; aucun bail court provoqué physiquement |
| Dernier produit installé, 25 octobre | 1 MiB via bridge/client/daemon installés, digest exact et imports vides | Smoke fonctionnel, aucune qualification de performance |
| Reconnexion, 24 octobre | Retrait USB : réseau absent ; activation manuelle du tethering : découverte et calcul rétablis | Aide installée et testée en logiciel, pas d'acceptation visuelle ; cas `NO_HINT` distinct |
| Dernière preuve physique, 30 octobre | Quarantaine native : refus, code de sortie 1, zéro job et zéro identité créée | Protection d'une expérience, aucun gain produit |

L'ancienne suite host de `39d097b` reste enregistrée **236 PASS / 1 FAIL**.
L'échec provenait de l'oracle de compteur OFF face à un Renew déjà admis ; le test
corrigé est passé ensuite isolément. Cette ancienne suite complète n'a pas été
requalifiée PASS. Les résultats des branches reconstruites sont des exécutions distinctes.

## Instrumentation et expériences laissées dans l'historique

Les bancs local/USB/Wi-Fi, traces opt-in et diagnostics restent séparés du produit.
Les preuves brutes, scripts de campagne et reprises spécialisées ne sont pas
publiés automatiquement. Le registre conserve leurs références historiques et
empreintes utiles ; elles désignent l'archive locale, pas des fichiers supposés
accessibles dans cette branche publique.

| Domaine | Verdict enregistré |
|---|---|
| SPEED BLAKE3 | Aucun gain qualifié ; distant 6,069 s contre local 0,405 s pour 64 MiB après polling |
| SPEED WebP | Aucun gain qualifié ; écran synthétique ADB, hors chemin produit chiffré |
| CAPACITY | `INCONCLUSIVE_PHYSICAL_TEST` ; campagne terminée, métrique primaire ambiguë et dérive de charge |
| THROUGHPUT | `THROUGHPUT_SCREEN_NOT_CONFIRMED` ; gains 18,98 %, −2,65 %, 16,87 %, seuil de 10 % non atteint dans 3/3 |
| CPU/SoC | Travail mono-thread observé, environ 0,994 cœur ; aucune preuve d'exploitation de tout le SoC |
| CPU multicœur | `NATIVE_SPIKE_QUARANTINED_HEALTH_SOURCE_NOT_EQUIVALENT` ; aucune rotation mesurée complète |
| Below-OS | `OPTIMIZE_USERSPACE_FIRST` ; étude terminée |

Le spike CPU est absent des branches produit. Dans l'archive, sa feature est
désactivée par défaut ; profils 1/2/4, 64 MiB par processus, plafond 128 MiB,
profil 4 refusé et maximum expérimental autorisable 2. Le collecteur confondait
`mLastMemoryLevel` avec `ActivityManager.MemoryInfo.lowMemory`, et Low Power
Standby avec `PowerManager.isPowerSaveMode`. L'entrée native refuse
`ANDROID_HEALTH_SOURCE_UNQUALIFIED` avant identités, sessions, auto-tests ou
enfants. Il ne faut ni réactiver ce collecteur ni contourner la quarantaine.

Les 31 commits originaux restent exclusivement sur la branche historique ; seuls
les sous-ensembles produit et les résumés explicitement décrits sont reconstruits.
Le registre machine donne les 30 classifications historiques plus le checkpoint
documentaire `9daae7f`, et précise le traitement des commits mixtes.

## Autorité canonique et reprises remplacées

Ordre inchangé : SPEC V0.7 ; Contract Set V1.3 ; Tech Sheet V1.3 ; Pseudocode
Critical Paths V1.1 ; Fixture Generation Spec V1.0. Les cinq originaux sont des
références externes, traitées en lecture seule selon les consignes de l'audit.
Leurs noms et empreintes vérifiées lors de
l'audit accepté sont dans [.pipeline/AUTHORITY.json](../../.pipeline/AUTHORITY.json).
Leurs emplacements internes et leur contenu ne sont pas publiés. Le pipeline ne
vérifie pas automatiquement ces fichiers externes ; un hash historique n'est pas
une vérification de disponibilité sur un nouveau clone.

Les décisions C12/C13/P14 déjà approuvées restent versionnées. Pour l'opération
BLAKE3 native C10, la baseline reste un thread et 8 MiB scratch, dans le plafond
total de 128 MiB. Les 64 MiB/processus du spike ne
redéfinissent pas C10. Aucun nouveau provider, mode SPEED, politique ResourceGuard
ou extension canonique n'est approuvé ici.

Ce checkpoint et les métadonnées de reprise associées remplacent les indications
suivantes comme ordres de travail ; les anciens rapports restent historiques :

- attente d'export de l'installateur : PR24 est déjà fusionnée ;
- absence absolue des cinq sources : elles étaient disponibles et hachées dans
  l'environnement de l'audit, sans être distribuées dans Git ;
- coupure USB présentée comme dernier verdict CAPACITY : campagne ultérieure
  complète, verdict inconclusif pour d'autres raisons ;
- attente d'implémentation du spike CPU : implémenté puis quarantiné ;
- correctif de bail présenté comme non déployé : déploiement et smoke installés
  enregistrés ; la perte initiale de transfert n'est pas déclarée expliquée ;
- reprise à `c0c6b04`, reprise directement du travail santé ou ancienne interdiction
  de toute publication : remplacées par l'intégration ordonnée #30, #27, #28, #29.

La référence locale de convergence `9daae7f` reste intacte ; elle n'est ni le HEAD
de `master` ni une nouvelle baseline publique. Les anciens fichiers `check-results`
et CI présents dans l'histoire ne valent pas validation des nouveaux commits.

## NEXT_ACTION

**Qualifier une source Android health correcte et réellement exploitable par
ResourceGuard, avant de reprendre le spike CPU multicœur et RAM distante.**

L'outillage pertinent et la CI ont été audités lors de #30. La qualification de
la santé Android est le seul chantier technique suivant ; cette mission de
convergence ne l'implémente pas.

Le blocage technique conservé est une source de santé Android fidèle, avec
équivalence des signaux requis, provenance, fraîcheur, gestion de l'absence et
refus fail-closed. GPU/NPU, Windows, below-OS, nouveau benchmark et refontes
génériques restent différés. Ne pas refaire les campagnes WebP/CAPACITY/THROUGHPUT,
l'audit mono-thread/SoC ni l'étude below-OS déjà archivés.
