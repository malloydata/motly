import { MOTLYNode, MOTLYDataNode, isRef, isEnvRef } from "../../interface/src/types";

/** Deep clone a MOTLYDataNode. */
export function cloneNode(value: MOTLYDataNode): MOTLYDataNode {
  const result: MOTLYDataNode = {};

  if (value.deleted) result.deleted = true;

  if (value.eq !== undefined) {
    if (value.eq instanceof Date) {
      result.eq = new Date(value.eq.getTime());
    } else if (Array.isArray(value.eq)) {
      result.eq = value.eq.map(cloneMotlyNode);
    } else if (isEnvRef(value.eq)) {
      result.eq = { env: value.eq.env };
    } else {
      result.eq = value.eq;
    }
  }

  if (value.properties) {
    const props: Record<string, MOTLYNode> = {};
    for (const key of Object.keys(value.properties)) {
      props[key] = cloneMotlyNode(value.properties[key]);
    }
    result.properties = props;
  }

  if (value.location) {
    result.location = {
      parseId: value.location.parseId,
      begin: { ...value.location.begin },
      end: { ...value.location.end },
    };
  }

  return result;
}

/** Deep clone a MOTLYNode (either a data node or a ref). */
export function cloneMotlyNode(node: MOTLYNode): MOTLYNode {
  if (isRef(node)) {
    return { linkTo: [...node.linkTo], linkUps: node.linkUps };
  }
  return cloneNode(node);
}

/** @deprecated Use cloneNode instead. */
export const cloneValue = cloneNode;

/** @deprecated Use cloneMotlyNode instead. */
export const clonePropertyValue = cloneMotlyNode;
