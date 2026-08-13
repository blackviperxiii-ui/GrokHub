import { openSettingsSection } from "./settings-nav";
import { useGrokHub } from "./store";

/** Start xAI device-code OAuth and open the approval URL. */
export async function startGrokOAuthAndOpenBrowser(): Promise<void> {
  await useGrokHub.getState().startGrokOAuth();
  const pending = useGrokHub.getState().oauthPending;
  const uri = pending?.verificationUriComplete || pending?.verificationUri;
  if (uri) window.open(uri, "_blank", "noopener,noreferrer");
}

/** Empty-chat / Offline banner: jump to Settings OAuth and start the flow. */
export async function beginGrokOAuthFromUi(): Promise<void> {
  useGrokHub.getState().setNav("settings");
  openSettingsSection("account", "sec-oauth");
  await startGrokOAuthAndOpenBrowser();
}
