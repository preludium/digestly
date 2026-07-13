// WebAuthn ceremony helpers (Stretch S1). `@github/webauthn-json` bridges the base64url JSON
// wire format used by `webauthn-rs` and the ArrayBuffer-based browser `navigator.credentials`
// API, so options from the server pass straight through and the resulting credential JSON goes
// straight back to the verify endpoint.

import {
    type CredentialCreationOptionsJSON,
    type CredentialRequestOptionsJSON,
    create,
    get,
    supported,
} from "@github/webauthn-json";

/** Whether this browser supports the WebAuthn APIs at all. */
export const passkeysSupported = () => supported();

/** Run a registration ceremony from server-issued creation options. */
export async function runRegistration(
    options: CredentialCreationOptionsJSON["publicKey"],
) {
    return create({ publicKey: options });
}

/** Run an authentication ceremony from server-issued request options. */
export async function runAuthentication(
    options: CredentialRequestOptionsJSON["publicKey"],
) {
    return get({ publicKey: options });
}

/**
 * Run a discoverable (Conditional UI / autofill) authentication ceremony. `mediation: "conditional"`
 * tells the browser to surface saved passkeys as autofill suggestions instead of a modal prompt.
 * The optional `signal` lets a background request be aborted if the user takes another action.
 */
export async function runConditionalAuthentication(
    options: CredentialRequestOptionsJSON["publicKey"],
    signal?: AbortSignal,
) {
    return get({ mediation: "conditional", publicKey: options, signal });
}

/** Whether the browser supports Conditional UI (autofill-driven passkey sign-in). */
export async function conditionalMediationAvailable(): Promise<boolean> {
    const pkc =
        typeof window !== "undefined" ? window.PublicKeyCredential : undefined;
    if (!pkc?.isConditionalMediationAvailable) return false;
    try {
        return await pkc.isConditionalMediationAvailable();
    } catch {
        return false;
    }
}

/** A user-cancelled or timed-out ceremony surfaces as one of these DOMException names. */
export function isCancellation(e: unknown): boolean {
    return (
        e instanceof DOMException &&
        (e.name === "NotAllowedError" || e.name === "AbortError")
    );
}
