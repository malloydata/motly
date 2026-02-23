import { MOTLYValue, MOTLYNode, isRef, isEnvRef } from "../../interface/src/types";

/** Deep clone a MOTLYValue. */
export function cloneValue(value: MOTLYValue): MOTLYValue {
  const result: MOTLYValue = {};

  if (value.deleted) result.deleted = true;

  if (value.eq !== undefined) {
    if (value.eq instanceof Date) {
      result.eq = new Date(value.eq.getTime());
    } else if (Array.isArray(value.eq)) {
      result.eq = value.eq.map(cloneValue);
    } else if (isRef(value.eq)) {
      result.eq = { linkTo: value.eq.linkTo };
    } else if (isEnvRef(value.eq)) {
      result.eq = { env: value.eq.env };
    } else {
      result.eq = value.eq;
    }
  }

  if (value.properties) {
    const props: Record<string, MOTLYNode> = {};
    for (const key of Object.keys(value.properties)) {
      props[key] = cloneValue(value.properties[key]);
    }
    result.properties = props;
  }

  return result;
}
