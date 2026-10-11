import { t } from "../i18n";

export function Dialog(name: string) {
  console.log("opening dialog");
  const cfg = { title: "Settings", message: "Delete {name}?", kind: "danger" };
  return <Button label="Save {name}" onClick={() => alert("Saved!")} />;
}
