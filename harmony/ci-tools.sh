#!/usr/bin/env bash
# For GitHub Actions: fetches Huawei's command-line tools for HarmonyOS from HARMONY_TOOLS_URL (the
# maintainer's own copy: Huawei's download needs a Huawei ID), checks the zip against
# HARMONY_TOOLS_SHA256, and puts the tools on the job's path: hvigorw and ohpm, DEVECO_SDK_HOME,
# OHOS_NDK_HOME and HAP_SIGN_TOOL. harmony/SIGNING.md says how to set the two.
set -euo pipefail
: "${HARMONY_TOOLS_URL:?HARMONY_TOOLS_URL (secret) isn't set: see harmony/SIGNING.md}"
: "${HARMONY_TOOLS_SHA256:?HARMONY_TOOLS_SHA256 (variable) isn't set: see harmony/SIGNING.md}"
curl -fsSL "$HARMONY_TOOLS_URL" -o "$RUNNER_TEMP/tools.zip"
echo "$HARMONY_TOOLS_SHA256  $RUNNER_TEMP/tools.zip" | sha256sum -c
unzip -q "$RUNNER_TEMP/tools.zip" -d "$RUNNER_TEMP/hos"
rm "$RUNNER_TEMP/tools.zip"
tools="$(dirname "$(find "$RUNNER_TEMP/hos" -maxdepth 3 -type f -name hvigorw | head -n 1)")/.."
tools="$(cd "$tools" && pwd)"
echo "$tools/bin" >>"$GITHUB_PATH"
echo "$tools/tool/node/bin" >>"$GITHUB_PATH"
echo "DEVECO_SDK_HOME=$tools/sdk" >>"$GITHUB_ENV"
echo "OHOS_NDK_HOME=$tools/sdk/default/openharmony" >>"$GITHUB_ENV"
echo "HAP_SIGN_TOOL=$tools/sdk/default/openharmony/toolchains/lib/hap-sign-tool.jar" >>"$GITHUB_ENV"
