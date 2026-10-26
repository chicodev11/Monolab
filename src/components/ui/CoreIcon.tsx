import { useState } from "react";
import { customProgram, findGame, type GameInfo } from "../../lib/kinds";
import { Icon } from "./Icon";
import { tr } from "../../lib/i18n";
import paperLogo from "../../assets/cores/papermc_logo.min.svg";
import velocityLogoWhite from "../../assets/cores/velocity_logo_white.min.svg";
import velocityLogoBlue from "../../assets/cores/velocity_logo_blue.min.svg";
import fabricLogo from "../../assets/cores/fabric_logo.png";
import forgeLogo from "../../assets/cores/forge_logo.svg";
import neoforgeLogo from "../../assets/cores/neoforge_logo.png";
import purpurLogo from "../../assets/cores/purpur_logo.svg";

export type CoreInfo = { icon: string; kind: string };
export const coreInfo: Record<string, CoreInfo> = {
  Vanilla: {
    icon: "vanilla",
    kind: tr("Default"),
  },
  Paper: {
    icon: "paper",
    kind: tr("Plugins"),
  },
  Purpur: {
    icon: "purpur",
    kind: tr("Plugins"),
  },
  Leaf: {
    icon: "leaf",
    kind: tr("Plugins"),
  },
  Folia: {
    icon: "folia",
    kind: tr("Plugins"),
  },
  Fabric: {
    icon: "fabric",
    kind: tr("Mods"),
  },
  Forge: {
    icon: "forge",
    kind: tr("Mods"),
  },
  NeoForge: {
    icon: "neoforge",
    kind: tr("Mods"),
  },
  Velocity: {
    icon: "velocity",
    kind: tr("Proxy"),
  },
  Pumpkin: {
    icon: "pumpkin",
    kind: tr("Beta"),
  },
};

const coreLogos: Record<string, { src: string; darkSrc?: string }> = {
  Paper: { src: paperLogo },
  Purpur: { src: purpurLogo },
  Fabric: { src: fabricLogo },
  Forge: { src: forgeLogo },
  NeoForge: { src: neoforgeLogo },
  Velocity: { src: velocityLogoBlue, darkSrc: velocityLogoWhite },
};

function CoreGlyph({ core }: { core: string }) {
  const p = {
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.6,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    "aria-hidden": true,
  };
  if (core === "Vanilla")
    return (
      <svg {...p}>
        <path d="M12 3.2 20.5 8v8L12 20.8 3.5 16V8z" />
        <path d="M3.5 8 12 12.6 20.5 8M12 12.6v8.2" />
      </svg>
    );
  if (core === "Folia")
    return (
      <svg {...p}>
        <path d="M4.5 19.5C4.5 11.4 11.4 4.5 19.5 4.5c0 8.1-6.9 15-15 15z" />
        <path d="M9 15.5c1.6-3.2 3.9-5.5 7-7" />
      </svg>
    );
  if (core === "Leaf")
    return (
      <svg {...p}>
        <path d="M12 3.5c4.6 3 6.4 7.2 5 11.2-.9 2.6-2.8 4.4-5 5-2.2-.6-4.1-2.4-5-5-1.4-4 .4-8.2 5-11.2z" />
        <path d="M12 8v13M12 12.5l2.6-2M12 16l-2.6-2" />
      </svg>
    );
  if (core === "Pumpkin")
    return (
      <svg {...p}>
        <path d="M12 6.6c-3.6 0-6 2.4-6 5.7s2.4 5.7 6 5.7 6-2.4 6-5.7-2.4-5.7-6-5.7z" />
        <path d="M12 6.6v11.4M8.6 7.6c-.9 1.3-1.3 2.9-1.3 4.7s.4 3.4 1.3 4.7M15.4 7.6c.9 1.3 1.3 2.9 1.3 4.7s-.4 3.4-1.3 4.7" />
        <path d="M12 6.6V4.4M12 4.4c1.1 0 2-.7 2-1.6" />
      </svg>
    );
  if (core === "BungeeCord")
    return (
      <svg {...p}>
        <path d="M10.2 14.4a4.2 4.2 0 0 1 0-5.9l2-2a4.2 4.2 0 0 1 5.9 5.9l-1 1" />
        <path d="M13.8 9.6a4.2 4.2 0 0 1 0 5.9l-2 2a4.2 4.2 0 0 1-5.9-5.9l1-1" />
      </svg>
    );
  return (
    <svg {...p}>
      <rect x="3.5" y="4.5" width="17" height="6.5" rx="1.8" />
      <rect x="3.5" y="13" width="17" height="6.5" rx="1.8" />
      <path d="M7 7.7h.01M7 16.2h.01" />
    </svg>
  );
}

function GameLogo({ game }: { game: GameInfo }) {
  const [broken, setBroken] = useState(false);
  if (broken) return <Icon name={game.icon} />;
  return <img className="core-logo game-logo" src={game.logo} alt="" draggable={false} onError={() => setBroken(true)} />;
}

const REMOTE_CORE_LOGOS: Record<string, string> = {
  Leaf: "https://www.leafmc.one/favicon.ico",
};

function RemoteCoreLogo({ core, src }: { core: string; src: string }) {
  const [broken, setBroken] = useState(false);
  if (broken) return <CoreGlyph core={core} />;
  return <img className="core-logo" src={src} alt="" draggable={false} onError={() => setBroken(true)} />;
}

const MINECRAFT_LOGO =
  "https://www.minecraft.net/etc.clientlibs/minecraftnet/clientlibs/clientlib-site/resources/favicon-96x96.png";

export function MinecraftLogo() {
  const [broken, setBroken] = useState(false);
  if (broken) return <Icon name="pickaxe" />;
  return <img className="core-logo game-logo" src={MINECRAFT_LOGO} alt="" draggable={false} onError={() => setBroken(true)} />;
}

export function CoreIcon({ core }: { core: string }) {
  if (core === customProgram.id) return <Icon name={customProgram.icon} />;
  const game = findGame(core);
  if (game) return <GameLogo game={game} />;
  const remote = REMOTE_CORE_LOGOS[core];
  if (remote) return <RemoteCoreLogo core={core} src={remote} />;
  const logo = coreLogos[core];
  if (!logo) return <CoreGlyph core={core} />;
  if (logo.darkSrc)
    return (
      <>
        <img className="core-logo on-light" src={logo.src} alt="" />
        <img className="core-logo on-dark" src={logo.darkSrc} alt="" />
      </>
    );
  return <img className="core-logo" src={logo.src} alt="" />;
}

