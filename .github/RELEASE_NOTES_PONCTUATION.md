Version de [Handy](https://github.com/cjpais/Handy) avec la **ponctuation dictée** : dites « virgule », « à la ligne »… et Handy écrit directement la ponctuation et les retours à la ligne au lieu des mots.

## Quel fichier télécharger ?

| Fichier | Pour quel Mac |
| --- | --- |
| `Handy_…_aarch64.dmg` | Mac Apple Silicon (puce M1, M2, M3, M4…) |
| `Handy_…_x64.dmg` | Mac Intel |

Pour savoir : menu  → **À propos de ce Mac** → ligne « Puce » (Apple M…) ou « Processeur » (Intel).

## Installation

1. Quittez Handy s'il est ouvert.
2. Ouvrez le `.dmg` et glissez **Handy** dans **Applications** (remplacez l'ancienne version). Vos réglages et modèles sont conservés.
3. **Premier lancement** : cette version n'est pas signée par Apple, macOS va la bloquer.
   - Allez dans **Réglages Système → Confidentialité et sécurité**, descendez et cliquez **Ouvrir quand même**.
   - Si macOS dit que l'app est « endommagée », ouvrez le Terminal et tapez :
     ```
     xattr -dr com.apple.quarantine /Applications/Handy.app
     ```
4. Autorisez à nouveau **Microphone** et **Accessibilité** quand Handy le demande. Si Handy reste bloqué sur « En attente… », retirez Handy de la liste **Accessibilité** (bouton −) puis rouvrez l'app.

## Commandes vocales

| Dites | Résultat |
| --- | --- |
| virgule | `,` |
| point / point final | `.` |
| point d'interrogation | ` ?` |
| point d'exclamation | ` !` |
| deux points | ` :` |
| point-virgule | ` ;` |
| points de suspension / trois petits points | `...` |
| à la ligne / retour à la ligne / nouvelle ligne / saut de ligne | retour à la ligne |
| nouveau paragraphe | ligne vide |
| ouvrez la parenthèse / fermez la parenthèse | `(` `)` |
| ouvrez les guillemets / fermez les guillemets | `«` `»` |

Exemple : « Bonjour virgule à la ligne je voulais te dire merci point » → 

```
Bonjour,
Je voulais te dire merci.
```

« point », « deux points », « nouvelle ligne » et « nouveau paragraphe » restent des mots normaux quand le contexte l'indique (« le point important », « point de vue », « une nouvelle ligne de bus »).

Réglage : **Paramètres → Avancé → Transcription → Ponctuation dictée** (activé par défaut).

## Mises à jour

Cette version ne se met pas à jour automatiquement depuis le Handy officiel (sinon la mise à jour effacerait la ponctuation dictée). Les nouvelles versions seront publiées sur cette page.
