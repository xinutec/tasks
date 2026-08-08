package org.xinutec.tasks

import org.xinutec.shell.ShellConfig
import org.xinutec.shell.WebShellActivity

/**
 * The task list — the Angular app served at [TASKS_URL], in the fleet's shared
 * [WebShellActivity]. Reachable over the VPN only and behind a Nextcloud
 * sign-in; the WebView keeps the session cookie, so it is a one-time login.
 *
 * There is nothing here but the address, which is the point: everything a
 * wrapper does belongs to `org.xinutec:shell`. This app has no deep link to
 * escape from and no state of its own — the page the shell already remembers is
 * the whole of it.
 *
 * ⚠ **An installed, signed-in copy is a standing authenticated session** over
 * the list, which is a working surface rather than a document: from it a task
 * can be moved, finished or filed. Same posture as the other wrappers, and the
 * phone is the device this was designed for.
 */
class MainActivity : WebShellActivity() {
    override val shell =
        ShellConfig(
            url = TASKS_URL,
            // The app plus the Nextcloud login hop. Without the second, the OAuth
            // round-trip is ejected to the browser and the app can never sign in;
            // everything else opens in the real browser.
            allowedHosts = setOf("tasks.xinutec.org", NC_HOST),
        )

    private companion object {
        // The task list (HTTPS, VPN-only DNS, behind a login).
        const val TASKS_URL = "https://tasks.xinutec.org/"

        // The Nextcloud identity provider the login bounces through.
        const val NC_HOST = "dash.xinutec.org"
    }
}
