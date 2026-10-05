# Connecting Google Tasks and Calendar (optional)

The to-do card can sync with **Google Tasks**, and the "Hari ini" column can show **today's Google Calendar events**
(read-only). This needs an OAuth client that belongs to **you**: nothing is shared with anyone else, there is no
server, and the sign-in token is kept in the Windows Credential Manager.

Use a personal Gmail account. Many school/work (Google Workspace) accounts block third-party apps.

## 1. Create the OAuth client (once, about 10 minutes)

1. Open <https://console.cloud.google.com/projectcreate>, sign in, and create a project (any name).
2. Enable two APIs: **Google Tasks API** and **Google Calendar API** (search them in "APIs & Services > Library").
3. Open **Google Auth Platform > Branding** (<https://console.cloud.google.com/auth/branding>) and fill the basics:
   an app name, your email as support email and as developer contact. Audience: **External**.
4. **Data access > Add or remove scopes**: add `.../auth/tasks` and `.../auth/calendar.readonly`, then Save.
5. **Clients > Create client**: type **Desktop app**. When the dialog shows the secret, click **Download JSON**.
   The file lands in your Downloads folder as `client_secret_....json`.

### Testing mode or In production?

- **Testing** (default): add your own email under *Audience > Test users*. Google expires the sign-in every **7 days**,
  so you sign in again weekly.
- **In production** (*Audience > Publish app*): the sign-in does not expire weekly, but Google asks for an app home page,
  privacy policy and terms links on a domain you own (a free GitHub Pages site works). You will see an "unverified app"
  screen when you sign in; that is normal for a personal app.

## 2. Connect it in the island

1. Open the island, go to the **Dashboard** tab. Next to **Tugas** there is a small status chip.
2. Click **Hubungkan Google**. The app imports the downloaded `client_secret_*.json` into the Windows
   Credential Manager and **deletes the file**.
3. Click **Masuk Google**. Your browser opens; choose your account, **Advanced > Go to ... (unsafe) > Allow**.
4. The chip turns into a green check. To-dos and today's events now sync (about every 90 seconds).

Stars on to-dos stay local (Google Tasks has no such thing). To disconnect, remove the app at
<https://myaccount.google.com/permissions>.
