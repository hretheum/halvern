# Signing and notarizing the macOS build

What has to happen before a `.dmg` can be downloaded by someone who is not you,
in the order it has to happen, with the parts only a human can do marked as
such.

Written 19 August 2026, when the Apple Developer account existed and the
certificate did not.

## The decision this all rests on

**Direct download, not the Mac App Store.** They are different projects, not two
settings.

Direct download needs a *Developer ID Application* certificate, notarization and
stapling. The codebase is already shaped for it: `hardenedRuntime` is on,
`entitlements.plist` covers microphone, audio, screen capture and calendars, and
there is deliberately no `com.apple.security.app-sandbox` key.

The Mac App Store needs a *Mac App Distribution* certificate and the App
Sandbox, which would have to be reconciled with capturing system audio through
ScreenCaptureKit, writing into `~/Movies`, running the `llama-helper` sidecar,
and downloading gigabytes of models on first launch. Each is solvable or fatal
depending on the entitlement Apple grants, and none of it is in place.

If that decision ever changes, this document does not describe the work.

## 1. Create the certificate — you, at the machine

Nobody can do this for you: it needs your Apple account.

**Through Xcode, which is the short way.** Xcode → Settings (⌘,) → **Accounts**
→ sign in → select the team → **Manage Certificates…** → **+** → **Developer ID
Application**. Xcode generates the signing request, submits it, and installs the
certificate with its private key. Three clicks and no files to shuffle.

**Through the portal, if Xcode will not.**

1. Keychain Access → Certificate Assistant → **Request a Certificate From a
   Certificate Authority**. Save to disk. This produces a `.certSigningRequest`
   and, importantly, puts the matching private key in your login keychain.
2. [developer.apple.com/account/resources/certificates](https://developer.apple.com/account/resources/certificates)
   → **+** → **Developer ID Application** → upload the CSR → download the
   `.cer`.
3. Double-click the `.cer` to install it.

**Pick the right team first.** More than one can be attached to an Apple ID —
typically a personal one and whichever paid membership actually exists. The
Developer ID option only appears under the team holding the membership, and only
for its **Account Holder**. If it is greyed out in Xcode you have the wrong team
selected or lack the role; the portal refuses for the same reason, so switching
tools does not help.

The Team ID is the `OU` field of any certificate already issued to that team,
not the value in parentheses after the name — those look identical and are
different things. On an *Apple Development* certificate the parenthetical is the
certificate's own identifier; on a *Developer ID Application* certificate it is
the Team ID, which is part of why they get confused.

To read it off a certificate you already have:

```bash
security find-certificate -c "Apple Development" -p \
  | openssl x509 -noout -subject | tr ',' '\n' | grep OU=
```

Check it landed:

```bash
security find-identity -v -p codesigning | grep "Developer ID Application"
```

You are looking for one line. Before this step that command prints nothing —
an *Apple Development* certificate is for running on your own devices and will
not do.

**Keep the private key.** The certificate is useless without it, and it exists
only in the keychain of the Mac that made the request. Export both together as a
`.p12` and store it somewhere you would still have after losing this machine.
Apple will not re-issue the key.

## 2. An app-specific password — you, in a browser

Notarization authenticates as you, and Apple will not accept your account
password.

[account.apple.com](https://account.apple.com) → Sign-In and Security →
App-Specific Passwords → generate one, label it something like `halvern
notarization`.

**Do not put it in a file in this repository**, and do not paste it into a
terminal that logs history. It goes in the environment for the build, and into
the CI secret store, and nowhere else.

## 3. Build signed and notarized — local

Three variables, set in the shell that runs the build:

```bash
# Read the identity exactly as the keychain spells it — do not retype it.
# Punctuation and non-ASCII characters in the organisation name have to match.
export APPLE_SIGNING_IDENTITY="$(security find-identity -v -p codesigning \
  | grep 'Developer ID Application' | head -1 | sed 's/.*"\(.*\)"/\1/')"

export APPLE_ID="you@example.com"
export APPLE_TEAM_ID="$(security find-certificate -c 'Developer ID Application' -p \
  | openssl x509 -noout -subject | sed -n 's/.*OU=\([A-Z0-9]*\).*/\1/p')"

read -rs APPLE_PASSWORD && export APPLE_PASSWORD   # typed, not echoed, not in history

cd frontend
node scripts/build-sidecar.js
pnpm run tauri:build
```

`tauri.conf.json` deliberately does **not** name the identity. It used to carry
`"signingIdentity": "-"`, the ad-hoc identity, which meant a build signed itself
locally and every credential in the environment was ignored — the log said
`Signing with identity "-"` and nobody read it. With the key absent Tauri takes
`APPLE_SIGNING_IDENTITY` from the environment, so an unsigned local build still
works and a signed one is a matter of exporting three variables.

Success looks like the absence of this line, which the unsigned build prints:

```
Warn skipping app notarization, no APPLE_ID & APPLE_PASSWORD & APPLE_TEAM_ID
```

Notarization takes minutes. Tauri staples the ticket when it returns.

**Then do the disk image separately, because Tauri does not.** It submits the
app, staples the app, and builds the `.dmg` around it — leaving the thing you
actually hand to people signed but unnotarized. Verified on 19 August: the app
came back `accepted / Notarized Developer ID` while the `.dmg` in the same build
came back `rejected / Unnotarized Developer ID`.

```bash
cd ../target/release/bundle/dmg
xcrun notarytool submit Halvern_0.4.0_aarch64.dmg \
  --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_PASSWORD" --wait
xcrun stapler staple Halvern_0.4.0_aarch64.dmg
```

Nothing is rebuilt; this only attaches a ticket. Stapling matters because it
makes the check work offline — without it Gatekeeper has to ask Apple, and a
user on a plane gets a warning for a file that is perfectly fine.

## 4. Verify — on a Mac that has never seen the app

This is the step people skip and it is the only one that proves anything. Done
for 0.4.0 on 19 August: downloaded onto a second machine, opened, launched, no
developer warning.

Gatekeeper does not quarantine what was built locally, so a broken signature
looks perfect on the machine that produced it. The check that matters is
downloading the `.dmg` over the network — not copying it over AirDrop from the
same iCloud account — onto a different Mac, then:

```bash
spctl -a -vvv -t install /Volumes/Halvern/Halvern.app
codesign --verify --deep --strict --verbose=2 /Volumes/Halvern/Halvern.app
xcrun stapler validate /path/to/Halvern_0.4.0_aarch64.dmg
```

`spctl` should say `accepted` and `source=Notarized Developer ID`. First launch
should ask for microphone and screen recording **without** a developer warning
in front of it.

## 5. CI — later, and not urgent

`.github/workflows/release.yml` has no macOS signing step. Adding one needs the
certificate exported as a base64 `.p12` and these repository secrets:

| Secret | What it is |
|---|---|
| `APPLE_CERTIFICATE` | the `.p12`, base64-encoded |
| `APPLE_CERTIFICATE_PASSWORD` | the password set when exporting it |
| `APPLE_SIGNING_IDENTITY` | the full identity string |
| `APPLE_ID` | the account email |
| `APPLE_PASSWORD` | the app-specific password from §2 |
| `APPLE_TEAM_ID` | the ten-character team identifier |

`tauri-action` imports the certificate into a temporary keychain when
`APPLE_CERTIFICATE` is present, so the workflow change is mostly passing these
through.

The `sign-binaries: true` already in that file is the Windows path and does
nothing for macOS.

This is deliberately last. Signing locally proves the certificate and the
entitlements work; automating it before that just moves a failure somewhere
harder to read.

## What is still true after all of this

The app is signed and notarized, which means macOS can verify it came from you
and has not been altered. It does **not** mean Apple reviewed it, and it does
not change anything in
[PRIVACY_POLICY.md](../PRIVACY_POLICY.md): a notarized build sends exactly as
much as an unsigned one, which is nothing.
