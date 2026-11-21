# Microsoft Entra ID sign-in and OneDrive backups

OpnRelay uses **one app registration** in your Microsoft Entra tenant for both
"Sign in with Microsoft" and OneDrive backups.

## 1. Register the app (Entra admin center)

1. **Identity → Applications → App registrations → New registration**
   - Name: `OpnRelay`
   - Supported account types: **Accounts in this organizational directory only**
   - Redirect URI: platform **Web**, value
     `https://<your OpnRelay address>/api/v1/auth/sso/microsoft/callback`
     (shown on *Admin → Microsoft sign-in* in OpnRelay)
2. Copy the **Application (client) ID** and **Directory (tenant) ID**.
3. **Certificates & secrets → New client secret.** Copy the *Value* and note the
   expiry date (renew it before it expires).
4. **API permissions → Add → Microsoft Graph → Delegated:** `openid`, `profile`,
   `email`, `offline_access`, `User.Read`, `Files.ReadWrite`. Then **Grant admin
   consent**.

## 2. Turn on sign-in (OpnRelay)

*Admin → Microsoft sign-in*: paste the tenant ID, client ID and secret, then:

| Option | Effect |
|---|---|
| Allow "Sign in with Microsoft" | Adds the button to the login page |
| Create accounts automatically | First sign-in creates the account with the chosen role (default *Member*); admins then add people to workspaces. Off = only invited people can sign in. |
| Microsoft sign-in only | Password sign-in is refused, except for **owners** (break-glass access if Microsoft is unavailable) |

Details:
- Accounts are matched by the Microsoft object ID, then by email address. Invited
  users who sign in with Microsoft are activated automatically.
- Only accounts from **your** tenant are accepted (the tenant ID is checked).
- Microsoft enforces your MFA and Conditional Access, so Microsoft sign-ins satisfy
  OpnRelay's "require two-factor" policy.
- The client secret is stored encrypted and never shown again.
- Planned: mapping Entra groups to OpnRelay groups/roles.

## 3. OneDrive backups

*Admin → Backups*:
1. Click **Connect OneDrive** and sign in with the Microsoft account whose OneDrive
   should hold the backups. A dedicated IT/service account is recommended, so
   backups don't depend on one employee.
2. Choose the folder (default `OpnRelay Backups`) and how many backups to keep there.
3. Backups run daily at the chosen hour (UTC), or use **Back up now**.

What is uploaded: the same encrypted `.orbk` file kept on the server, containing the
database (guides, steps, users, settings, audit log) and all screenshots. OneDrive
only ever receives ciphertext. Decrypting needs this server's **master key**
(`/opt/opnrelay/secrets/master_key`), so keep a copy of the key somewhere safe and
separate from OneDrive.

Network: the server must reach `login.microsoftonline.com` and `graph.microsoft.com`
over HTTPS.

## 4. Restoring

On a new server, run the installer with the backup and the saved master key:

```bash
curl -fsSL https://raw.githubusercontent.com/stevy2191/opnrelay/main/deploy/install.sh | \
  sudo OPNRELAY_RESTORE=/root/opnrelay-backup-20261121-020000.orbk \
       OPNRELAY_RESTORE_KEY=/root/master_key bash
```

The installer sets up OpnRelay with the old key, then loads the backup. Users sign in
with their existing accounts (and Microsoft sign-in keeps working if the new server
uses the same address).
