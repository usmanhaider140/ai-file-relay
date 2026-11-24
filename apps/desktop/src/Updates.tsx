// Checks GitHub releases for a newer signed version, a few seconds after
// start and then every 6 hours, and offers to install it.

import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

const EVERY = 6 * 60 * 60 * 1000;

export function UpdateBanner() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [progress, setProgress] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const look = () =>
      check()
        .then((u) => setUpdate(u))
        // No network, or no release yet: try again later, quietly.
        .catch((e) => console.info("update check:", e));
    const first = setTimeout(look, 5000);
    const again = setInterval(look, EVERY);
    return () => {
      clearTimeout(first);
      clearInterval(again);
    };
  }, []);

  if (!update) return null;

  const install = async () => {
    setError(null);
    let total = 0;
    let done = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          done += event.data.chunkLength;
          setProgress(total ? `${Math.round((done / total) * 100)}%` : "downloading…");
        }
        if (event.event === "Finished") setProgress("installing…");
      });
      await relaunch();
    } catch (e) {
      setProgress(null);
      setError(String(e));
    }
  };

  return (
    <div className="notice">
      <span>
        {error
          ? `The update failed: ${error}`
          : progress
            ? `Updating to ${update.version}: ${progress}`
            : `Version ${update.version} is ready.`}
      </span>
      {!progress && (
        <button className="primary" onClick={install}>
          Install and restart
        </button>
      )}
    </div>
  );
}
