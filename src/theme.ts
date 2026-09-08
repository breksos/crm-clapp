import { useCallback, useEffect, useState } from "react";

/**
 * The theme, which is **local state and never shared state**.
 *
 * `docs/architecture.md` §6 makes the view shared because both surfaces have to agree on
 * *what is being looked at*. How it is painted is not that: the agent does not care what
 * theme the person is in, and putting it in the snapshot would make a colour scheme
 * something an agent could change out from under someone mid-drag.
 *
 * So it lives in `localStorage`, and `localStorage` is allowed to say no — a private
 * window, cleared site data, a browser set to block storage. Every read and write is
 * wrapped, and the un-stored answer ("system") is a real, correct state rather than a
 * fallback.
 */
export type Theme = "system" | "light" | "dark";

const KEY = "breksos.theme";

function read(): Theme {
  try {
    const v = localStorage.getItem(KEY);
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
  }
}

/** Three states, not two. An explicit choice stamps `data-theme` on the root; "system"
 *  **removes** the stamp, so `prefers-color-scheme` decides and the bare `:root` palette
 *  is what applies. That un-stamped state is the one a token defined only inside a media
 *  query breaks, which is why it is a first-class option and not the absence of one. */
function stamp(theme: Theme): void {
  const root = document.documentElement;
  if (theme === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", theme);
}

export function useTheme(): [Theme, (t: Theme) => void] {
  const [theme, setTheme] = useState<Theme>(read);

  useEffect(() => {
    stamp(theme);
  }, [theme]);

  const choose = useCallback((next: Theme) => {
    setTheme(next);
    try {
      localStorage.setItem(KEY, next);
    } catch {
      // The choice still applies for this session; it just will not survive a restart.
    }
  }, []);

  return [theme, choose];
}
