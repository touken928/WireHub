import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Field,
  Input,
  Select,
  Text,
  tokens,
} from '@fluentui/react-components';

export type CreatePeerDialogGroup = {
  id: number;
  name: string;
};

type CreatePeerDialogProps = {
  open: boolean;
  name: string;
  groupId: string;
  groups: CreatePeerDialogGroup[];
  error?: string;
  onNameChange: (value: string) => void;
  onGroupChange: (value: string) => void;
  onClose: () => void;
  onCreate: () => void;
};

export function CreatePeerDialog({
  open,
  name,
  groupId,
  groups,
  error,
  onNameChange,
  onGroupChange,
  onClose,
  onCreate,
}: CreatePeerDialogProps) {
  return (
    <Dialog open={open} onOpenChange={(_, data) => !data.open && onClose()}>
      <DialogSurface>
        <DialogBody>
          <DialogTitle>New peer</DialogTitle>
          <DialogContent>
            <Field label="Group" required>
              <Select value={groupId} onChange={(_, data) => onGroupChange(data.value)}>
                {groups.map((group) => (
                  <option key={group.id} value={String(group.id)}>{group.name}</option>
                ))}
              </Select>
            </Field>
            <Field label="Peer name" required hint="Used as hostname (e.g. laptop)">
              <Input value={name} onChange={(_, data) => onNameChange(data.value)} />
            </Field>
            {error ? (
              <Text size={200} style={{ color: tokens.colorPaletteRedForeground1 }}>{error}</Text>
            ) : null}
          </DialogContent>
          <DialogActions>
            <Button onClick={onClose}>Cancel</Button>
            <Button
              appearance="primary"
              disabled={!name.trim() || !groupId}
              onClick={onCreate}
            >
              Create
            </Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
