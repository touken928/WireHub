export type GroupNodeData = {
  label: string;
  groupId: number;
};

export type LinkDrawMode = 'bidirectional' | 'unidirectional';

export type GroupLinkEdgeData = {
  fromGroupId: number;
  toGroupId: number;
  bidirectional: boolean;
};
