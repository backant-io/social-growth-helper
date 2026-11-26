import type { SectionId } from "../lib/types";

export interface Section {
  id: SectionId;
  label: string;
  title: string;
  empty: string;
  phase: number;
}

export const SECTIONS: Section[] = [
  { id: "scout", label: "Scout Queue", title: "Scout Queue", empty: "Posts worth answering will appear here once Scout is connected to your X window.", phase: 3 },
  { id: "reply", label: "Reply Studio", title: "Reply Studio", empty: "Pick a post from the Scout Queue to draft a reply in your voice.", phase: 4 },
  { id: "creator", label: "Creator", title: "Creator", empty: "Start an original post from a topic or a viral format.", phase: 5 },
  { id: "analytics", label: "Analytics", title: "Analytics", empty: "Your numbers show up here after the first analytics sync.", phase: 6 },
  { id: "learning", label: "Learning", title: "Learning", empty: "Weekly insights appear after you have posted and Analytics has synced.", phase: 7 },
  { id: "style", label: "Style", title: "Style", empty: "Add examples of your writing and the app will describe your voice.", phase: 4 },
  { id: "settings", label: "Settings", title: "Settings", empty: "", phase: 1 },
];
