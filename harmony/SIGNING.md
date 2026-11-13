# Signing and releasing ProCraft for HarmonyOS

The release workflow's `harmony` job (`.github/workflows/release.yml`) builds the HarmonyOS app
and, as the Mac job does, signs it in CI from GitHub secrets. It is skipped until set up. This is
what sets it up and what a signed `.hap` can and can't do.

**Signing is optional.** With only the command-line tools set up (steps 1, 2 and 4: a free Huawei
ID, no certificates), the job releases `procraft-<version>-harmonyos-arm64-unsigned.hap`. That
installs once someone signs it with their own Huawei account (harmony/README.md, "Install the
unsigned build yourself"). Steps 3 and 5's signing secrets switch it to a signed `.hap`. Huawei
issues the certificates, to a verified account.

Confidence: **High** = Huawei's or OpenHarmony's own documentation; **Medium** = documentation that
leaves gaps, or several consistent secondary sources; **Low** = one secondary source, or untested.

## Why the Android and Mac keys can't be used

HarmonyOS installs a package only if its signing certificate was issued by Huawei (AppGallery
Connect issues it for your key's certificate request) and its profile, which Huawei signs, binds
that certificate to the app's bundle name (and, for a debug profile, to the devices allowed). A
certificate of our own making, the Android keystore's or the Developer ID's, chains to nothing a
HarmonyOS device trusts. The key itself is new too: the signing tool signs with ECDSA (P-256 or
P-384), and the Android key is RSA. (High: OpenHarmony's hapsigntool guide; AppGallery Connect's
FAQ.)

## Steps

1. **Account.**
   - Make a Huawei ID, register on developer.huawei.com and complete identity verification.
   - An individual verifies with an ID document and a bank card.
   - Without verification, debug certificates last 14 days instead of a year.
   - (Medium: Huawei's developer privacy statement; AppGallery Connect FAQ.)
2. **DevEco Studio:**
   - Install it on a drive other than C: (the installer is about 3 GB; Huawei asks for plenty of
     free disk), and sign in with the Huawei ID.
   - Then Build > Generate Key and CSR.
     - It makes `procraft.p12`, the keystore: choose its password, the key's alias (e.g.
       `procraft`) and the key's password.
     - It also makes `procraft.csr`, the certificate request.
     - Keep the `.p12` and the passwords as safe as the Android release key.
   - (High: DevEco Studio's signing guide.)
3. **AppGallery Connect** (developer.huawei.com > AppGallery Connect > Certificates, APP IDs and
   Profiles):
   - **APP ID:** a new one for a HarmonyOS app, bundle name `io.github.artcraftandroid.procraft`
     (the same as the Android and iPad apps).
   - **Certificates:** a **debug certificate** and a **release certificate**, each from
     `procraft.csr`; download both `.cer` files.
   - **Devices:** each tester's device by its UDID. With developer mode on, `hdc shell bm get
     --udid` reads it; or the device's Settings > About.
   - **Profiles:** a **debug profile** (the APP ID, the debug certificate, the testers' devices)
     and a **release profile** (the APP ID, the release certificate); download both `.p7b` files.
   - (Medium: AppGallery Connect's help; the exact menu names may differ by language.)
4. **Command-line tools:**
   - Download *Command Line Tools for HarmonyOS*, the Linux zip, from developer.huawei.com's
     download page (it needs the Huawei ID). The CI runner builds with them: hvigorw, ohpm, the
     SDK with its clang and hap-sign-tool.
   - Put the zip somewhere private that `curl` can fetch without other headers, for example a
     link with a token in it. This is the URL in `HARMONY_TOOLS_URL`.
   - Note its SHA-256 (`sha256sum` or `Get-FileHash`).
   - Read the licence you accept at the download first: keeping a private copy for your own CI
     has to be allowed by it. It wasn't checked here (unverified).
5. **GitHub**, in the public repository's Settings > Secrets and variables > Actions. The
   maintainer's `set-harmony-secrets.sh` sets these from files: the tools' two always, the
   signing ones only when all six files are there.

   | Name | Kind | What |
   |---|---|---|
   | `HARMONY_TOOLS_URL` | secret | the command-line tools zip's private URL (needed) |
   | `HARMONY_TOOLS_SHA256` | variable | its SHA-256 (needed) |
   | `HARMONY_P12` | secret | `procraft.p12`, base64 (this and the five below: signed builds only) |
   | `HARMONY_CER` | secret | a `.cer`, base64 (see below for which) |
   | `HARMONY_P7B` | secret | the matching `.p7b`, base64 |
   | `HARMONY_KEY_ALIAS` | secret | the key's alias |
   | `HARMONY_KEY_PASSWORD` | secret | the key's password |
   | `HARMONY_STORE_PASSWORD` | secret | the keystore's password |
   | `RELEASE_HARMONY` | variable | `true`, to build the job at all |

   - Base64 in Git Bash: `base64 -w0 procraft.p12`. In PowerShell:
     `[Convert]::ToBase64String([IO.File]::ReadAllBytes("procraft.p12"))`.
   - Or set them all with `gh secret set HARMONY_P12 < <(base64 -w0 procraft.p12)` and so on.
6. **A dry run** (Actions > Release > Run workflow) builds the `.hap` (artifact
   `release-harmonyos`: signed with the signing secrets, else unsigned; signed, the unsigned one
   comes too, as `harmonyos-unsigned`). A tag then drafts the release with
   `procraft-<version>-harmonyos-arm64.hap` (or `…-unsigned.hap`).

## Which certificate and profile go into the secrets

**The debug pair (the debug `.cer` and the debug `.p7b`), for a GitHub download.** A
release-signed `.hap` installs through AppGallery only (or enterprise distribution); downloaded
and installed by hand, the system refuses it. A debug-signed one installs, but only on the
devices whose UDIDs are in its profile. (Medium: AppGallery Connect FAQ, "an app can be installed
only on a device whose UDID has been added to the debug profile"; that a release-signed `.hap`
can't be installed by hand comes from several secondary sources.)

So the honest choices for a download are:

- **Debug-signed, for listed testers.** Each tester sends their UDID, it is added to the debug
  profile, and the profile is downloaded again into `HARMONY_P7B`.
  - Testers turn on developer mode and install with `hdc install procraft-….hap` (DevEco
    Studio's command-line tools, or any hdc).
  - AppGallery Connect limits how many devices an account registers; reports say 100 (Low).
  - The certificate lasts a year, and the profile has its own expiry; after that a new download
    is needed.
- **Unsigned, for people who sign it themselves.** The dry run's `…-unsigned.hap` installs nowhere
  as it is.
  - Someone with their own Huawei ID can sign it with their own debug certificate and profile
    (their own device's UDID in it): with `hap-sign-tool sign-app` (the job's command, with
    their files), or with tools that wrap it.
  - Or they can build from source in DevEco Studio (harmony/README.md), which signs
    automatically for their device. Either way it takes a developer account and developer mode.
  - (Medium: OpenHarmony's hapsigntool guide for the command; community tools for the
    practice.)
- **AppGallery** (a release-signed `.app`, not a `.hap`): the store's review, a privacy policy,
  and for mainland China, where HarmonyOS NEXT is sold, an app filing (APP备案), which needs a
  Chinese entity or agent. The job doesn't build the `.app` yet (`hvigorw assembleApp`, signed by
  hvigor from a signing config in `build-profile.json5`). (Medium: harmony/PLAN.md §6.)

## Sources

- [OpenHarmony: hapsigntool guide](https://gitcode.com/openharmony/docs/blob/master/en/application-dev/security/hapsigntool-guidelines.md)
  (sign-app, ECDSA only, localSign).
- [OpenHarmony: application package overview](https://gitcode.com/openharmony/docs/blob/master/en/application-dev/quick-start/application-package-overview.md)
  (.hap, .har, .hsp, App Pack).
- [AppGallery Connect FAQ](https://developer.huawei.com/consumer/en/doc/app/agc-help-releasefaq-0000001110342644)
  (certificate validity, debug profiles and UDIDs).
- [Huawei: building from the command line](https://developer.huawei.com/consumer/en/doc/harmonyos-guides/ide-command-line-building-app)
  (hvigorw assembleHap and assembleApp, JDK, libGL).
- [Huawei developer privacy statement](https://developer.huawei.com/consumer/en/devservice/term)
  (what identity verification asks for).
