// Content script, injected into a page only when an upload is approved.
//
// Receives the file in pieces from the background worker, then puts it into
// the page: into a file input (like a person choosing a file), or, for pages
// that only have a drag-and-drop area, by simulating a drop.
//
// This file must stay a plain script (no import/export): Chrome injects it
// with chrome.scripting.executeScript.

(() => {
  const flag = window as unknown as { __aiFileRelay?: boolean };
  if (flag.__aiFileRelay) return;
  flag.__aiFileRelay = true;

  interface Transfer {
    name: string;
    mime: string;
    selector: string | null;
    parts: Uint8Array<ArrayBuffer>[];
  }
  type Answer = { ok: true } | { ok: false; error: string };

  const transfers = new Map<string, Transfer>();

  chrome.runtime.onMessage.addListener((msg, _sender, reply: (a: Answer) => void) => {
    if (!msg || typeof msg.kind !== "string" || !msg.kind.startsWith("relay_")) return false;
    try {
      reply(handle(msg));
    } catch (e) {
      reply({ ok: false, error: String(e) });
    }
    return false;
  });

  function handle(msg: {
    kind: string;
    transfer: string;
    name?: string;
    mime?: string;
    selector?: string | null;
    data?: string;
  }): Answer {
    switch (msg.kind) {
      case "relay_begin":
        transfers.set(msg.transfer, {
          name: msg.name ?? "file",
          mime: msg.mime ?? "application/octet-stream",
          selector: msg.selector ?? null,
          parts: [],
        });
        return { ok: true };
      case "relay_chunk": {
        const t = transfers.get(msg.transfer);
        if (!t) return { ok: false, error: "unknown transfer" };
        t.parts.push(fromBase64(msg.data ?? ""));
        return { ok: true };
      }
      case "relay_end": {
        const t = transfers.get(msg.transfer);
        transfers.delete(msg.transfer);
        if (!t) return { ok: false, error: "unknown transfer" };
        const file = new File(t.parts, t.name, { type: t.mime });
        return place(file, t.selector);
      }
      default:
        return { ok: false, error: `unknown message ${msg.kind}` };
    }
  }

  function fromBase64(data: string): Uint8Array<ArrayBuffer> {
    const binary = atob(data);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
    return bytes;
  }

  /** querySelectorAll that also looks inside open shadow roots. */
  function deepQueryAll(selector: string, root: ParentNode = document): Element[] {
    const found = Array.from(root.querySelectorAll(selector));
    for (const el of Array.from(root.querySelectorAll("*"))) {
      if (el.shadowRoot) found.push(...deepQueryAll(selector, el.shadowRoot));
    }
    return found;
  }

  function isVisible(el: Element): boolean {
    const r = el.getBoundingClientRect();
    const style = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && style.visibility !== "hidden" && style.display !== "none";
  }

  function findTarget(selector: string | null): Element | null {
    if (selector) return deepQueryAll(selector)[0] ?? null;
    const inputs = deepQueryAll('input[type="file"]').filter(
      (el) => !(el as HTMLInputElement).disabled,
    );
    // Many sites hide the real input behind a nice button, so a hidden
    // input is fine; a visible one is still preferred.
    return inputs.find(isVisible) ?? inputs[0] ?? null;
  }

  function place(file: File, selector: string | null): Answer {
    const target = findTarget(selector);
    if (!target) {
      return {
        ok: false,
        error: selector
          ? `nothing on the page matches the selector ${selector}`
          : "no file upload box found on the page. Pass a selector for the file input or drop zone.",
      };
    }
    const dt = new DataTransfer();
    dt.items.add(file);

    if (target instanceof HTMLInputElement && target.type === "file") {
      if (target.disabled) return { ok: false, error: "the file input is disabled" };
      target.files = dt.files;
      target.dispatchEvent(new Event("input", { bubbles: true }));
      target.dispatchEvent(new Event("change", { bubbles: true }));
      return { ok: true };
    }

    // A drag-and-drop area.
    for (const type of ["dragenter", "dragover", "drop"]) {
      target.dispatchEvent(
        new DragEvent(type, { bubbles: true, cancelable: true, dataTransfer: dt }),
      );
    }
    return { ok: true };
  }
})();
