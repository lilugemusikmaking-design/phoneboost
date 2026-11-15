# Reprise : comparaison C12 release/dev ABAB terminée

Branche `spike/c12-end-to-end-timing-v0-1`, PR #33 ouverte, non fusionnée.
Source produit mesurée : `31ecca48a347a95c1650cf08822e82cf82889e88`.
Base master : `524d0065f5609919c61e37c242771ee3bf61eb9b`.
Résoudre le HEAD publié et ses checks sur GitHub ; ne pas reprendre l'ancien
NEXT_ACTION demandant de compiler en release.

[Rapport](../../docs/architecture/C12_RELEASE_VS_DEV_V0_1.md) —
[preuves](../../docs/evidence/c12-release-vs-dev-v0-1.json).

Deux warmups puis trois paires dev/release exactes : **34,855 → 3,586 s**,
**9,720×**, 3/3 favorables. Noise **32,490 s → 90,160 ms**. Le profil Cargo
release standard est la seule différence logicielle. Environnement Cargo,
resolver/features snow et état CPU/alimentation host sont enregistrés.
Trois premières mesures release groupées et trois dev historiques conservées
séparément : 26,502 → 3,563 s (7,439×). Ne pas refaire ces campagnes closes.

Le nouveau poste dominant est **SessionPoll host, 3,325 s médian**. Il contient
les sleeps inconditionnels de 20 ms et l'attente de traitement/réponse Android.
Ce n'est pas un RTT USB. La part évitable n'est pas encore isolée.

Battery Saver ON, profil 1, collecte OFF, app/daemon/forward expérimentaux
arrêtés, APK produit et expérimental inchangés ; restauration finale vérifiée.
Aucune modification Noise/PBMUX/chunks/protocole/ResourceGuard. Pas de chantier
RAM/GPU/NPU/C13, d'affinité, ni de suppression de worktree historique.

NEXT_ACTION : Réduire la latence du polling de session à 20 ms sur le chemin PUT en release, avec mesure A/B et protocole inchangé.
