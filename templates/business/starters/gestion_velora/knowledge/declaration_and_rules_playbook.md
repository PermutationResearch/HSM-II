# Playbook — réponses fondées sur la déclaration et le règlement

## Principe (non négociable)

L’assistant **ne « sait » pas** ce qui est vrai pour **votre** immeuble sans les **textes intégrés** dans `integrated_governing_docs.md` (ou fichiers équivalents que vous listez dans `knowledge_files`).  
**Toute réponse** sur droits / interdits / procédures doit : **(1)** s’appuyer sur un passage **cité** (article, section, page si utile), ou **(2)** dire clairement que ce n’est **pas** dans les documents fournis et proposer la **marche à suivre** (vérification auprès du gestionnaire, conseil syndical, avis juridique).

## Hiérarchie des sources (ordre de référence)

1. **Déclaration de copropriété** (acte constitutif, états descriptifs, cadastre si inclus dans vos extraits).  
2. **Règlement de copropriété / règles du bâtiment** (bruit, animaux, stationnement, parties communes, travaux, etc.).  
3. **Baux / politiques locataires** (pour les **locataires** : d’abord bail + règlement qui lie le locataire, pas toute la déclaration sauf si le texte le prévoit).  
4. **Politiques internes Velora** (délais de réponse, formulaires) — distinctes des obligations légales ; ne pas les confondre avec la loi.

## Format de réponse recommandé

- **Constat** : reformuler la question (copropriétaire vs locataire, unité si connue).  
- **Réponse** : « Selon le [règlement / déclaration], [article X] : … » (citation courte ou renvoi précis).  
- **Si absent des documents** : « Ce point n’apparaît pas dans les extraits fournis. Nous vérifions auprès de … » (pas d’invention).  
- **Si interprétation juridique** : escalade « avis juridique / conseil syndical » sans trancher.

## Types de demandes fréquentes (comment les traiter)

| Thème | Copropriétaire | Locataire |
|--------|----------------|-----------|
| Bruit / voisinage | Règlement + déclaration (usage des parties privées/communes) | Règlement + bail |
| Animaux | Règlement / déclaration | Idem si applicable au locataire |
| Stationnement | Déclaration (parking) + règlement | Bail + règlement |
| Travaux / rénovation | Procédure syndicat, règlement, déclaration (permutations) | Autorisation propriétaire + règlement |
| Parties communes / accès | Déclaration + règlement | Règlement + contact propriétaire/gestion |
| Charges / budget | Déclaration + PV / budget si fournis | Rarement — renvoi au bail / propriétaire |
| Assemblée / vote | Déclaration + loi (ne pas interpréter : résumer processus général seulement si dans docs) | N/A en général |
| Urgence (fuite, sécurité) | Procédure urgence Velora + règlement | Idem + numéros d’urgence fournis |

## Ce qu’il ne faut jamais faire

- Inventer un **article**, un **pourcentage**, une **amende** ou une **procédure** non écrite dans les documents fournis.  
- Dire « c’est légal / illégal » sur une **interprétion** fine — renvoyer à un humain qualifié.  
- Mélanger **deux immeubles** : si plusieurs syndicats, exiger l’identification du syndicat / adresse avant de citer un texte.

## Mise à jour des documents

Quand la déclaration ou le règlement change : **mettre à jour** les extraits dans `integrated_governing_docs.md`, ajuster `last_reviewed` dans `pack.yaml`, et noter la date de révision dans les documents.
