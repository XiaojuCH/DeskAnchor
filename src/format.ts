export function desktopSummary(monitorCount: number, iconCount: number): string {
  const monitorLabel = monitorCount === 1 ? "display" : "displays";
  const iconLabel = iconCount === 1 ? "icon" : "icons";
  return `${monitorCount} ${monitorLabel} · ${iconCount} ${iconLabel}`;
}

export function snapshotTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}
