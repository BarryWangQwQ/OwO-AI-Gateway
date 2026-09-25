// UI icon set: Keyline Icons (MIT, https://keylineicons.com), stroke style,
// rounded corners.
//
// App code — including the shadcn primitives under ui/ — imports icons from
// "@/components/icons" under the names used throughout the codebase, so
// swapping the icon set or tuning its look is a change to this file only.
// Keyline draws at strokeWidth 2 on a 24×24 grid and each generated component
// spreads `fill / stroke / strokeWidth / strokeLinecap / strokeLinejoin` and
// then `...props` onto the <svg>, so anything passed here overrides the
// built-in defaults and is inherited by every <path> inside. Sub-paths that are
// pure fills (dots, sparkles) set `stroke="none"` themselves and are unaffected.
//
// The svg carries no class / data attribute of its own, so a global CSS rule
// would have to match bare `svg` and leak into recharts and brand marks
// (VendorIcon). Hence the tiny HOC below instead of a stylesheet.
//
// Brand marks (VendorIcon / @lobehub/icons-static-svg) are not part of this set.

import type { ComponentType, SVGProps } from "react";
import * as K from "@keyline-icons/react";
import { Save as LucideSave } from "lucide-react";

export type { IconProps } from "@keyline-icons/react";

/** Single knob for the whole app's icon weight. Keyline/lucide default is 2. */
export const ICON_STROKE_WIDTH = 2.4;

type StrokeProps = Pick<SVGProps<SVGSVGElement>, "strokeWidth" | "strokeLinecap" | "strokeLinejoin">;

const ICON_STYLE = {
  strokeWidth: ICON_STROKE_WIDTH,
  strokeLinecap: "round",
  strokeLinejoin: "round",
} satisfies StrokeProps;

/**
 * Wrap an icon component so it renders with the app-wide stroke defaults.
 * Props passed by the caller still win (`<X strokeWidth={1.5} />`).
 */
function withStyle<P extends StrokeProps>(Icon: ComponentType<P>): ComponentType<P> {
  const Styled = (props: P) => <Icon {...ICON_STYLE} {...props} />;
  Styled.displayName = Icon.displayName ?? Icon.name;
  return Styled;
}

// Same name in both sets.
export const Activity = withStyle(K.Activity);
export const AppWindow = withStyle(K.AppWindow);
export const ArrowRight = withStyle(K.ArrowRight);
export const Check = withStyle(K.Check);
export const ChevronDown = withStyle(K.ChevronDown);
export const ChevronRight = withStyle(K.ChevronRight);
export const ChevronUp = withStyle(K.ChevronUp);
export const CircleArrowUp = withStyle(K.CircleArrowUp);
export const CircleCheck = withStyle(K.CircleCheck);
export const CircleDollarSign = withStyle(K.CircleDollarSign);
export const CircleSlash = withStyle(K.CircleSlash);
export const CircleX = withStyle(K.CircleX);
export const Code = withStyle(K.Code);
export const Compass = withStyle(K.Compass);
export const Coins = withStyle(K.Coins);
export const DatabaseZap = withStyle(K.DatabaseZap);
export const Download = withStyle(K.Download);
export const File = withStyle(K.File);
export const FilePlus = withStyle(K.FilePlus);
export const FileText = withStyle(K.FileText);
export const Flag = withStyle(K.Flag);
export const Folder = withStyle(K.Folder);
export const FolderOpen = withStyle(K.FolderOpen);
export const FolderPlus = withStyle(K.FolderPlus);
export const FolderTree = withStyle(K.FolderTree);
export const FolderZip = withStyle(K.FolderZip);
export const GitBranch = withStyle(K.GitBranch);
export const Globe = withStyle(K.Globe);
export const Info = withStyle(K.Info);
export const KeyRound = withStyle(K.KeyRound);
export const LayoutDashboard = withStyle(K.LayoutDashboard);
export const Link2 = withStyle(K.Link2);
export const Link2Off = withStyle(K.Link2Off);
export const LoaderCircle = withStyle(K.LoaderCircle);
export const Lock = withStyle(K.Lock);
export const Monitor = withStyle(K.Monitor);
export const Moon = withStyle(K.Moon);
export const MoreHorizontal = withStyle(K.MoreHorizontal);
export const OctagonX = withStyle(K.OctagonX);
export const PackagePlus = withStyle(K.PackagePlus);
export const PanelLeft = withStyle(K.PanelLeft);
export const Plug = withStyle(K.Plug);
export const Plus = withStyle(K.Plus);
export const Power = withStyle(K.Power);
export const Radio = withStyle(K.Radio);
export const RefreshCw = withStyle(K.RefreshCw);
export const RotateCcw = withStyle(K.RotateCcw);
export const Search = withStyle(K.Search);
export const Server = withStyle(K.Server);
export const Settings = withStyle(K.Settings);
export const Sparkles = withStyle(K.Sparkles);
export const SquarePen = withStyle(K.SquarePen);
export const Sun = withStyle(K.Sun);
export const Terminal = withStyle(K.Terminal);
export const TriangleAlert = withStyle(K.TriangleAlert);
export const Unlock = withStyle(K.Unlock);
export const WandSparkles = withStyle(K.WandSparkles);
export const X = withStyle(K.X);

// Keyline name differs from the lucide name the codebase uses.
export const BarChart3 = withStyle(K.ChartColumn);
export const Box = withStyle(K.Package);
export const Boxes = withStyle(K.Layers);
export const History = withStyle(K.ClockArrowLeft);
export const Languages = withStyle(K.Language);
export const Pencil = withStyle(K.Pen);
export const Trash2 = withStyle(K.Bin);

// No Keyline equivalent (no floppy-disk glyph in 1.2.0); keep the single lucide
// icon rather than a semantically wrong one. Drop this line once Keyline ships one.
// lucide also draws at strokeWidth 2 on 24×24 and spreads props onto the <svg>.
export const Save = withStyle(LucideSave);
