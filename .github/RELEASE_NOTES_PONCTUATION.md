Version de [Handy](https://github.com/cjpais/Handy) avec la **ponctuation dictée** : dites « virgule », « à la ligne »… et Handy écrit directement la ponctuation et les retours à la ligne au lieu des mots.

## Déjà installé ? Mettez à jour depuis Handy

Si vous avez déjà une version **0.9.6005 ou plus récente**, rien à télécharger ici : en bas de la fenêtre de Handy, cliquez sur **« Mise à jour disponible »** (ou « Rechercher des mises à jour »). Handy se met à jour et redémarre tout seul, vos autorisations sont conservées.

## Première installation

### Quel fichier télécharger ?

Dans la liste des fichiers en bas de cette page (**Assets**) :

| Fichier | Pour quel Mac |
| --- | --- |
| `Handy_…_aarch64.dmg` | Mac Apple Silicon (puce M1, M2, M3, M4…) |
| `Handy_…_x64.dmg` | Mac Intel |

Pour savoir : menu  → **À propos de ce Mac** → ligne « Puce » (Apple M…) ou « Processeur » (Intel).

Les autres fichiers (`.tar.gz`, `latest.json`, « Source code ») servent aux mises à jour automatiques : ne les téléchargez pas.

### Installation

1. Quittez Handy s'il est ouvert (icône en haut à droite de l'écran → Quitter).
2. Ouvrez le `.dmg` et glissez **Handy** dans **Applications** (cliquez **Remplacer** si demandé). Vos réglages et modèles sont conservés, pas besoin de désinstaller l'ancienne version.
3. **Premier lancement** : macOS affiche « Élément "Handy" non ouvert ». Cliquez **Terminé** (surtout pas « Placer dans la corbeille »), puis allez dans **Réglages Système → Confidentialité et sécurité**, descendez et cliquez **Ouvrir quand même**.
   - Si macOS dit que l'app est « endommagée », ouvrez le Terminal et tapez :
     ```
     xattr -dr com.apple.quarantine /Applications/Handy.app
     ```
4. Autorisez **Microphone** et **Accessibilité** quand Handy le demande. Si Handy reste sur « En attente… » pour l'accessibilité, collez ceci dans le Terminal puis réautorisez :
   ```
   osascript -e 'tell application id "com.pais.handy" to quit'; tccutil reset Accessibility com.pais.handy; open /Applications/Handy.app
   ```

Ces étapes ne sont à faire qu'une fois : les mises à jour suivantes se font depuis Handy et gardent les autorisations.

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

Exemple : « Bonjour virgule à la ligne je voulais te dire merci point à la ligne cordialement » →

```
Bonjour,
Je voulais te dire merci.
Cordialement
```

« point », « deux points », « nouvelle ligne » et « nouveau paragraphe » restent des mots normaux quand le contexte l'indique (« le point important », « point de vue », « une nouvelle ligne de bus »).

Réglage : **Paramètres → Avancé → Transcription → Ponctuation dictée** (activé par défaut).
