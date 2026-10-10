import { invokeBridge } from "./bridge";

export function getAutostart(): Promise<boolean> {
  return invokeBridge<boolean>("get_autostart");
}

export function setAutostart(enabled: boolean): Promise<boolean> {
  return invokeBridge<boolean>("set_autostart", { enabled });
}
