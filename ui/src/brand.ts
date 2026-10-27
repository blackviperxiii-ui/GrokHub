// The product's name, tagline, companion and art, from brand/brand.json and
// the files next to it. Every visible name in the UI comes from here, so a
// rename is one line in brand.json and new art is a file swap.
import brand from "../../brand/brand.json";
import logoMark from "../../brand/logo-mark.svg";
import appIcon from "../../brand/app-icon.svg";

export const PRODUCT_NAME: string = brand.productName;
export const TAGLINE: string = brand.tagline;
export const COMPANION_NAME: string = brand.companionName;
/** Settings → About: the quiet link shown once a support URL is set. */
export const SUPPORT_LABEL: string = brand.supportLabel;

/** Image URLs for the logo mark and the app icon tile. */
export const LOGO_MARK_URL: string = logoMark;
export const APP_ICON_URL: string = appIcon;
