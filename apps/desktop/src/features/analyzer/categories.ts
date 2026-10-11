/** Category labels and colors (design tokens only: the `--hue-*` gradients). */
export const CATEGORY_LABEL: Record<string, string> = {
  image: "Images",
  video: "Videos",
  audio: "Audio",
  document: "Documents",
  pdf: "PDFs",
  spreadsheet: "Spreadsheets",
  presentation: "Presentations",
  code: "Code",
  data: "Data",
  archive: "Archives",
  installer: "Installers",
  font: "Fonts",
  model3d: "3D models",
  game_asset: "Game assets",
  executable: "Programs",
  system: "System files",
  other: "Other",
};

const HUE: Record<string, string> = {
  image: "pink",
  video: "violet",
  audio: "indigo",
  document: "blue",
  pdf: "red",
  spreadsheet: "green",
  presentation: "amber",
  code: "teal",
  data: "teal",
  archive: "amber",
  installer: "slate",
  font: "slate",
  model3d: "indigo",
  game_asset: "violet",
  executable: "red",
  system: "slate",
  other: "slate",
};

export const categoryLabel = (c: string) => CATEGORY_LABEL[c] ?? c;
export const categoryBackground = (c: string) => `var(--hue-${HUE[c] ?? "slate"})`;

export function download(file: { file_name: string; mime: string; contents: string }) {
  const url = URL.createObjectURL(new Blob([file.contents], { type: file.mime }));
  const a = document.createElement("a");
  a.href = url;
  a.download = file.file_name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
