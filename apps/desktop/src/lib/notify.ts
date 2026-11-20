/** Native notifications through the webview, used only when the core did not show one itself. */

const available = (): boolean => typeof Notification !== "undefined";

/** Ask once, from a user gesture such as setting a reminder; never throws. */
export async function requestNotificationPermission(): Promise<void> {
  if (!available() || Notification.permission !== "default") return;
  try {
    await Notification.requestPermission();
  } catch {
    // Some webviews do not implement the prompt; reminders still show in the app.
  }
}

/** Show a system notification. Returns false when the system does not allow one. */
export function showNativeNotification(title: string, body: string): boolean {
  if (!available() || Notification.permission !== "granted") return false;
  try {
    new Notification(title, { body });
    return true;
  } catch {
    return false;
  }
}
