import { MOTLYNode, MOTLYPropertyValue, isRef, isEnvRef } from "../../interface/src/types";

/** Deep clone a MOTLYNode. */
export function cloneNode(value: MOTLYNode): MOTLYNode {
  const result: MOTLYNode = {};

  if (value.deleted) result.deleted = true;

  if (value.eq !== undefined) {
    if (value.eq instanceof Date) {
      result.eq = new Date(value.eq.getTime());
    } else if (Array.isArray(value.eq)) {
      result.eq = value.eq.map(clonePropertyValue);
    } else if (isEnvRef(value.eq)) {
      result.eq = { env: value.eq.env };
    } else {
      result.eq = value.eq;
    }
  }

  if (value.properties) {
    const props: Record<string, MOTLYPropertyValue> = {};
    for (const key of Object.keys(value.properties)) {
      props[key] = clonePropertyValue(value.properties[key]);
    }
    result.properties = props;
  }

  return result;
}

/** Deep clone a MOTLYPropertyValue (either a node or a ref). */
export function clonePropertyValue(pv: MOTLYPropertyValue): MOTLYPropertyValue {
  if (isRef(pv)) {
    return { linkTo: pv.linkTo };
  }
  return cloneNode(pv);
}

/** @deprecated Use cloneNode instead. */
export const cloneValue = cloneNode;
