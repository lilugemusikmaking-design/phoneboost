# Battery Saver pendant les campagnes de développement

`scripts/physical_campaign_power.py` gère la bascule réversible autour d'une
commande de campagne. `phoneboost_perfetto_bench/run_campaign.sh` l'utilise
automatiquement, avant de démarrer Perfetto ou un workload. Le corps de capture
est conservé dans `capture_profile.sh` ; événements, buffer, durée et commandes
de calcul sont inchangés.

Pour une autre campagne, avec le service Health de l'APK expérimental déjà actif :

```sh
python3 scripts/physical_campaign_power.py \
  --state /tmp/campagne-privee/battery-saver.json -- python3 campagne.py
```

Le wrapper lit `settings global low_power` et exige un échantillon officiel
PhoneBoost cohérent et frais. Il mémorise l'état initial dans un nouveau journal
privé (0600), lié au hash de l’identité ADB du téléphone, appelle `adb shell cmd power set-mode 0` si initialement ON, puis
attend un **nouvel** échantillon `power_save=false` et le réglage système concordant.
Un retour ADB zéro ne suffit pas. Aucun test n'est lancé si cette vérification échoue.
L'âge Health est calculé uniquement avec BOOTTIME Android (limite existante 6 s).
La fenêtre d'observation du wrapper est bornée à 30 s par défaut ; aucun timeout
PhoneBoost n'est modifié. Le préflight complet ResourceGuard reste obligatoire
dans la commande de campagne.

Le `finally` restaure exactement l'état initial, y compris après échec ou interruption,
et vérifie ensuite Health. Le script attend/termine uniquement son propre groupe
de processus enfant avant cette restauration. Le service Android doit rester actif
jusqu'à la fin du wrapper ; arrêter ensuite l'APK expérimental. Les codes non nuls
du test sont conservés. Une restauration non confirmée produit l'erreur explicite
`BATTERY_SAVER_RESTORATION_FAILED` et le code 4, même si le test avait réussi.

Un SIGKILL, une coupure du host ou un débranchement peut empêcher l'exécution du
finally. La cible ADB est fixée au téléphone initial ; une reprise sur un autre téléphone est refusée.
Le journal n'est jamais écrasé par une nouvelle campagne. Après reconnexion
et redémarrage du service Health, sans lancer de workload :

```sh
python3 scripts/physical_campaign_power.py --restore /tmp/campagne-privee/battery-saver.json
```

## Vérification réelle du 15 novembre 2026

Sur le Samsung SM-A155F Android 16 de cette mission, `cmd power set-mode 0` a été
accepté par ADB mais **n'a pas désactivé** Battery Saver : Health est resté à true.
La commande témoin n'a donc jamais été exécutée, sortie 2. Le finally a confirmé
l'état initial ON par Health frais (653 ms). Aucune mesure 64 MiB supplémentaire.
Le wrapper est automatique et fail-closed ; la bascule automatique réussie n'est
**pas qualifiée sur ce Samsung**. Aucun fallback par UI ou écriture forcée de réglage
n'est introduit, et aucune confirmation manuelle n'est demandée par le script.

L'application produit ne pilote jamais Battery Saver et conserve ses règles de
fonctionnement conservateur. Le wrapper ne change ni ResourceGuard ni les seuils.
