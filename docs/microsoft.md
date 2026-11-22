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
   `email`, `offline_access`, `User.Read`, `Files.ReadWrite`.
   Only if you want group sync (section 3), also add **Application** permissions
   `GroupMember.Read.All` and `User.Read.All`. Both are read-only. Then **Grant
   admin consent**.

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
- To limit who can sign in, open **Enterprise applications → OpnRelay → Properties**,
  set **Assignment required?** to **Yes**, then add the allowed users or groups under
  **Users and groups**. Everyone else is refused by Microsoft.

## 3. Group sync

Group sync keeps OpnRelay groups, roles and leavers in step with Microsoft. It is
optional and off by default.

**Turn it on.** Add the two application permissions in step 1.4 and grant admin
consent. Group sync needs your **specific tenant ID**: `organizations`, `common` and
`consumers` do not work. Then tick the group sync option on *Admin → Microsoft
sign-in*, save, and use **Sync now** to test.

**Link groups.** *Admin → Groups → Add from Microsoft*: search by name and pick a
group. Members come from Microsoft, including members of nested groups, and are
read-only in OpnRelay. If the name clashes with an existing OpnRelay group, the
linked group is named "<name> (Microsoft)". People appear as members once they have
signed in with Microsoft.

**When it runs.**
- Every hour (the scheduler checks every 5 minutes).
- When a person signs in with Microsoft (for that person).
- When you link a group.
- When you click **Sync now**.

If your groups come from on-premises Active Directory through Entra Connect, Entra
Connect copies changes about every 30 minutes. Worst case, a change takes about 90
minutes to show up in OpnRelay.

**Roles.** Each linked group has *Also make members: nothing / auditor / admin*. If
someone is in several groups, the highest role wins. Owners are never changed.
While any mapping exists, the roles of Microsoft-linked people are set by Microsoft
and locked in the *Users* tab. When you remove the last mapping, they go back to
manual management and keep their current role. Before you save a mapping, the
confirmation shows how many people will change role.

**Leavers.** If a person's Microsoft account is disabled or deleted (none of their
linked Microsoft accounts is enabled), OpnRelay disables them, revokes all their
sessions and desktop/extension sign-ins, and removes them from Microsoft groups.
This is written to the audit log. Owners are never disabled; a warning is shown
instead. They are re-enabled automatically only when they sign in again with the same
Microsoft account, and only if Microsoft (not an admin) disabled them. Disabled
people, whether disabled by an admin or by Microsoft, are not members of Microsoft
groups until they are re-enabled.

**Safety brake.** If one run would disable more than 10 people *and* more than 20%
of Microsoft-linked people, it disables no one and shows a banner. This usually
means something is wrong in Microsoft (for example, a bulk change). Check Microsoft
first. An admin can then confirm that one run, and only up to the number shown.

**When something goes wrong.**
- If Microsoft can't be reached or the client secret has expired, nothing changes and
  admins see an error banner. After a day without a successful run the banner adds
  "No successful sync for over 24 hours". Sign-in still works with existing access.
  Renew the secret and run **Sync now**.
- If a group is deleted in Microsoft, it is emptied and shown in red so you can
  unlink it.
- If you turn group sync off, linked groups are frozen and show "Not syncing".

## 4. OneDrive backups

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

## 5. Restoring

On a new server, run the installer with the backup and the saved master key:

```bash
curl -fsSL https://raw.githubusercontent.com/stevy2191/opnrelay/main/deploy/install.sh | \
  sudo OPNRELAY_RESTORE=/root/opnrelay-backup-20261121-020000.orbk \
       OPNRELAY_RESTORE_KEY=/root/master_key bash
```

The installer sets up OpnRelay with the old key, then loads the backup. Users sign in
with their existing accounts (and Microsoft sign-in keeps working if the new server
uses the same address).
