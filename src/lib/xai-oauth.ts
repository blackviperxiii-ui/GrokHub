/**
 * xAI Grok OAuth (device-code) — same public client used by Grok CLI / OpenClaw.
 * SuperGrok or X Premium+ accounts get API access tokens without a console API key.
 * Node-only (server / Electron main).
 */

export const XAI_OAUTH_CLIENT_ID = "b1a00492-073a-47ea-816f-4c329264a828";
export const XAI_OAUTH_SCOPE =
  "openid profile email offline_access grok-cli:access api:access";
export const XAI_OAUTH_ISSUER = "https://auth.x.ai";
export const XAI_OAUTH_DISCOVERY = `${XAI_OAUTH_ISSUER}/.well-known/openid-configuration`;
export const XAI_DEVICE_CODE_GRANT =
  "urn:ietf:params:oauth:grant-type:device_code";
export const XAI_UA = "GrokHub/0.2.2 (xAI OAuth; Linux)";

export type XaiOAuthTokens = {
  accessToken: string;
  refreshToken?: string;
  expiresAt?: number;
  idToken?: string;
  email?: string;
  name?: string;
  picture?: string;
  connectedAt: number;
};

export type DeviceCodeStart = {
  deviceCode: string;
  userCode: string;
  verificationUri: string;
  verificationUriComplete?: string;
  expiresIn: number;
  interval: number;
};

type Discovery = {
  deviceAuthorizationEndpoint: string;
  tokenEndpoint: string;
  userinfoEndpoint: string;
};

function formBody(data: Record<string, string>): string {
  return new URLSearchParams(data).toString();
}

function trustedXai(url: string): string {
  const u = new URL(url);
  if (u.protocol !== "https:") throw new Error("xAI OAuth requires https");
  if (u.hostname !== "x.ai" && !u.hostname.endsWith(".x.ai")) {
    throw new Error(`Untrusted xAI host: ${u.hostname}`);
  }
  return url;
}

function decodeJwtPayload(token: string): Record<string, unknown> {
  try {
    const part = token.split(".")[1];
    if (!part) return {};
    const json = Buffer.from(part, "base64url").toString("utf8");
    return JSON.parse(json) as Record<string, unknown>;
  } catch {
    return {};
  }
}

function pickPicture(...candidates: unknown[]): string | undefined {
  for (const c of candidates) {
    if (typeof c === "string" && /^https?:\/\//i.test(c.trim())) return c.trim();
  }
  return undefined;
}

function pickName(...candidates: unknown[]): string | undefined {
  for (const c of candidates) {
    if (typeof c === "string" && c.trim()) return c.trim();
  }
  return undefined;
}

async function discovery(): Promise<Discovery> {
  const res = await fetch(XAI_OAUTH_DISCOVERY, {
    headers: { accept: "application/json", "user-agent": XAI_UA },
  });
  if (!res.ok) throw new Error(`xAI OIDC discovery failed (${res.status})`);
  const j = (await res.json()) as Record<string, unknown>;
  const device = j.device_authorization_endpoint;
  const token = j.token_endpoint;
  const userinfo = j.userinfo_endpoint;
  if (typeof device !== "string" || typeof token !== "string") {
    throw new Error("xAI discovery missing device/token endpoints");
  }
  return {
    deviceAuthorizationEndpoint: trustedXai(device),
    tokenEndpoint: trustedXai(token),
    userinfoEndpoint:
      typeof userinfo === "string" ? trustedXai(userinfo) : `${XAI_OAUTH_ISSUER}/oauth2/userinfo`,
  };
}

function parseTokens(json: Record<string, unknown>): Omit<XaiOAuthTokens, "connectedAt"> {
  const accessToken = json.access_token;
  if (typeof accessToken !== "string" || !accessToken) {
    throw new Error("Token response missing access_token");
  }
  const refreshToken =
    typeof json.refresh_token === "string" && json.refresh_token
      ? json.refresh_token
      : undefined;
  const idToken =
    typeof json.id_token === "string" && json.id_token ? json.id_token : undefined;
  let expiresAt: number | undefined;
  if (typeof json.expires_in === "number") {
    expiresAt = Date.now() + json.expires_in * 1000;
  } else if (typeof json.expires_in === "string" && /^\d+$/.test(json.expires_in)) {
    expiresAt = Date.now() + Number(json.expires_in) * 1000;
  }
  return { accessToken, refreshToken, idToken, expiresAt };
}

export async function startXaiDeviceCode(): Promise<DeviceCodeStart> {
  const d = await discovery();
  const res = await fetch(d.deviceAuthorizationEndpoint, {
    method: "POST",
    headers: {
      "content-type": "application/x-www-form-urlencoded",
      accept: "application/json",
      "user-agent": XAI_UA,
    },
    body: formBody({
      client_id: XAI_OAUTH_CLIENT_ID,
      scope: XAI_OAUTH_SCOPE,
    }),
  });
  const json = (await res.json().catch(() => ({}))) as Record<string, unknown>;
  if (!res.ok) {
    const msg =
      (typeof json.error_description === "string" && json.error_description) ||
      (typeof json.error === "string" && json.error) ||
      `device code failed (${res.status})`;
    throw new Error(msg);
  }
  const deviceCode = String(json.device_code || "");
  const userCode = String(json.user_code || "");
  const verificationUri = String(json.verification_uri || "");
  if (!deviceCode || !userCode || !verificationUri) {
    throw new Error("Invalid device code response from xAI");
  }
  trustedXai(verificationUri);
  const verificationUriComplete =
    typeof json.verification_uri_complete === "string"
      ? trustedXai(json.verification_uri_complete)
      : undefined;
  return {
    deviceCode,
    userCode,
    verificationUri,
    verificationUriComplete,
    expiresIn: typeof json.expires_in === "number" ? json.expires_in : 1800,
    interval: typeof json.interval === "number" ? json.interval : 5,
  };
}

export type PollResult =
  | { status: "pending"; error?: string }
  | { status: "slow_down"; interval?: number }
  | { status: "expired" | "denied"; error: string }
  | { status: "ready"; tokens: XaiOAuthTokens };

export async function pollXaiDeviceCode(deviceCode: string): Promise<PollResult> {
  const d = await discovery();
  const res = await fetch(d.tokenEndpoint, {
    method: "POST",
    headers: {
      "content-type": "application/x-www-form-urlencoded",
      accept: "application/json",
      "user-agent": XAI_UA,
    },
    body: formBody({
      grant_type: XAI_DEVICE_CODE_GRANT,
      client_id: XAI_OAUTH_CLIENT_ID,
      device_code: deviceCode,
    }),
  });
  const json = (await res.json().catch(() => ({}))) as Record<string, unknown>;
  if (res.ok && typeof json.access_token === "string") {
    const base = parseTokens(json);
    let email: string | undefined;
    let name: string | undefined;
    let picture: string | undefined;

    // id_token claims (often include profile picture from Google/X)
    if (base.idToken) {
      const claims = decodeJwtPayload(base.idToken);
      email = pickName(claims.email) || email;
      name =
        pickName(claims.name, claims.preferred_username, claims.given_name) || name;
      picture = pickPicture(claims.picture, claims.avatar_url, claims.profile_image_url);
    }

    try {
      const ui = await fetchUserinfo(base.accessToken, d.userinfoEndpoint);
      email = ui.email || email;
      name = ui.name || name;
      picture = ui.picture || picture;
    } catch {
      /* optional */
    }

    return {
      status: "ready",
      tokens: {
        ...base,
        email,
        name,
        picture,
        connectedAt: Date.now(),
      },
    };
  }

  const err = typeof json.error === "string" ? json.error : "unknown";
  if (err === "authorization_pending") return { status: "pending", error: err };
  if (err === "slow_down") return { status: "slow_down" };
  if (err === "expired_token" || err === "access_denied") {
    return {
      status: err === "expired_token" ? "expired" : "denied",
      error:
        (typeof json.error_description === "string" && json.error_description) || err,
    };
  }
  return {
    status: "pending",
    error:
      (typeof json.error_description === "string" && json.error_description) ||
      `waiting (${res.status})`,
  };
}

async function fetchUserinfo(
  accessToken: string,
  endpoint: string,
): Promise<{ email?: string; name?: string; picture?: string }> {
  const res = await fetch(endpoint, {
    headers: {
      authorization: `Bearer ${accessToken}`,
      accept: "application/json",
      "user-agent": XAI_UA,
    },
  });
  if (!res.ok) return {};
  const j = (await res.json()) as Record<string, unknown>;
  return {
    email: pickName(j.email),
    name: pickName(j.name, j.preferred_username, j.given_name),
    picture: pickPicture(j.picture, j.avatar_url, j.profile_image_url, j.image),
  };
}

export async function refreshXaiOAuth(
  refreshToken: string,
): Promise<Omit<XaiOAuthTokens, "connectedAt">> {
  const d = await discovery();
  const res = await fetch(d.tokenEndpoint, {
    method: "POST",
    headers: {
      "content-type": "application/x-www-form-urlencoded",
      accept: "application/json",
      "user-agent": XAI_UA,
    },
    body: formBody({
      grant_type: "refresh_token",
      client_id: XAI_OAUTH_CLIENT_ID,
      refresh_token: refreshToken,
    }),
  });
  const text = await res.text();
  if (!res.ok) {
    if (/cloudflare|<!doctype html/i.test(text)) {
      throw new Error(
        "xAI blocked token refresh (Cloudflare). Re-run Grok OAuth sign-in.",
      );
    }
    let msg = `refresh failed (${res.status})`;
    try {
      const j = JSON.parse(text) as { error?: string; error_description?: string };
      msg = j.error_description || j.error || msg;
    } catch {
      /* ignore */
    }
    throw new Error(msg);
  }
  const json = JSON.parse(text) as Record<string, unknown>;
  return parseTokens(json);
}

/** Resolve a usable access token, refreshing if near expiry. */
export async function ensureAccessToken(tokens: XaiOAuthTokens): Promise<{
  accessToken: string;
  tokens: XaiOAuthTokens;
  refreshed: boolean;
}> {
  if (!tokens?.accessToken) {
    throw new Error("No OAuth access token — connect Grok OAuth in Settings");
  }
  const skew = 60_000;
  const expired =
    typeof tokens.expiresAt === "number" && tokens.expiresAt - skew < Date.now();
  if (!expired) {
    return { accessToken: tokens.accessToken, tokens, refreshed: false };
  }
  if (!tokens.refreshToken) {
    throw new Error("Grok OAuth session expired — sign in again");
  }
  const next = await refreshXaiOAuth(tokens.refreshToken);
  const merged: XaiOAuthTokens = {
    ...tokens,
    accessToken: next.accessToken,
    refreshToken: next.refreshToken || tokens.refreshToken,
    expiresAt: next.expiresAt,
    idToken: next.idToken || tokens.idToken,
  };
  return { accessToken: merged.accessToken, tokens: merged, refreshed: true };
}
