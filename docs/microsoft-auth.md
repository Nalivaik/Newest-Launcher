# Microsoft authentication

`Newest Launcher` does not use shared or copied Client IDs and never asks for a
Microsoft password inside the application. A real Minecraft Java sign-in needs
an Application (client) ID owned by the launcher publisher. This ID is public;
it is **not** a client secret and must not be treated as one.

Before enabling the Microsoft account flow, register the desktop application in
Microsoft Entra:

1. Create an **App registration** named `Newest Launcher`.
2. Select **Accounts in any organizational directory and personal Microsoft
   accounts** (or *Personal Microsoft accounts only* if that is the intended
   audience). Minecraft/Xbox accounts are normally personal Microsoft accounts.
3. In **Authentication**, add the **Mobile and desktop applications** platform
   with the system-browser redirect URI `http://localhost` and enable public
   client flows if using device code.
4. Record the **Application (client) ID**. Do not create or embed a client
   secret in this desktop app: desktop clients use PKCE/device-code as public
   clients.
5. Provide that Client ID to the launcher configuration used for the release.

The implemented authentication chain is deliberately strict:

`Microsoft OAuth → Xbox Live → XSTS → Minecraft services → entitlements → Minecraft profile`

Only after the entitlement response confirms Minecraft Java ownership may the
launcher create a `microsoft` profile. The profile then contains the Minecraft
name, UUID and official skin URL. OAuth, Xbox and Minecraft tokens are stored
only in the OS credential store (Windows Credential Manager on Windows), never
in `state.json`, browser storage, logs or crash reports.

The current registration is personal-account-only, so the launcher uses the
official Microsoft `consumers` device-code endpoint. It opens the system browser
and shows the one-time code inside the account dialog. This is a public-client
flow: no client secret is embedded in the application.

A **Local / Offline profile** is a different saved entity: its nickname is
entered by the player, its UUID is local, it has no skin/token/entitlement, and
the UI labels it `offline`. It must never be sent as a Microsoft session or
claimed to work on authenticated online servers.

References: [Microsoft device authorization flow](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code), [desktop app configuration](https://learn.microsoft.com/en-us/entra/identity-platform/scenario-desktop-app-configuration), and [supported account types](https://learn.microsoft.com/en-us/graph/auth-register-app-v2).
