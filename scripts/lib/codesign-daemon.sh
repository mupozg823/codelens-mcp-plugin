# shellcheck shell=bash
# Sign the CodeLens daemon binary. Sourced by redeploy-daemons.sh and
# install-http-daemons-launchd.sh.
#
# By default the binary is signed ad hoc. macOS keys an ad-hoc signature to
# the binary's content hash, so every rebuild is a new app to TCC: the first
# file open in ~/Downloads, ~/Documents or ~/Desktop after a redeploy shows a
# consent dialog and blocks until someone answers it (2026-10-10: a bind held
# in open() for 234.8 s, and again for over ten minutes, waiting on "...이(가)
# 다운로드 폴더의 파일에 접근하려고 합니다").
#
# Set CODELENS_CODESIGN_IDENTITY to a code-signing identity in your keychain
# (its name or SHA-1, as `security find-identity -v -p codesigning` prints it)
# to sign with that certificate and a fixed identifier instead. The consent
# then survives rebuilds. An identity that is missing or fails to sign is an
# error: falling back to ad hoc would bring the dialog back silently.

CODELENS_CODESIGN_IDENTIFIER="${CODELENS_CODESIGN_IDENTIFIER:-dev.codelens.mcp-http}"

codelens_sign_daemon() {
	local bin="$1"
	local identity="${CODELENS_CODESIGN_IDENTITY:-}"
	if ! command -v codesign >/dev/null 2>&1; then
		return 0
	fi
	if [[ -z "${identity}" ]]; then
		echo "==> ad-hoc signing ${bin} (macOS will ask again for Downloads/Documents/Desktop access; set CODELENS_CODESIGN_IDENTITY to keep it, see docs/operations/http-daemon.md)"
		codesign --force --sign - "${bin}" || {
			echo "warning: codesign failed; daemon may be killed by Gatekeeper" >&2
		}
		return 0
	fi
	if ! security find-identity -v -p codesigning 2>/dev/null | grep -F -- "${identity}" >/dev/null; then
		echo "error: CODELENS_CODESIGN_IDENTITY='${identity}' is not a valid code-signing identity in the keychain" >&2
		echo "       list them with: security find-identity -v -p codesigning" >&2
		return 1
	fi
	echo "==> signing ${bin} with '${identity}' as ${CODELENS_CODESIGN_IDENTIFIER}"
	if ! codesign --force --sign "${identity}" --identifier "${CODELENS_CODESIGN_IDENTIFIER}" "${bin}"; then
		echo "error: codesign with '${identity}' failed" >&2
		return 1
	fi
	codesign --verify --strict "${bin}" || {
		echo "error: codesign --verify failed for ${bin}" >&2
		return 1
	}
}
