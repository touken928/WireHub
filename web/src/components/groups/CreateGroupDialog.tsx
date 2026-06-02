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
} from '@fluentui/react-components';

type CreateGroupDialogProps = {
  open: boolean;
  name: string;
  onNameChange: (value: string) => void;
  onClose: () => void;
  onCreate: () => void;
};

export function CreateGroupDialog({
  open,
  name,
  onNameChange,
  onClose,
  onCreate,
}: CreateGroupDialogProps) {
  return (
    <Dialog open={open} onOpenChange={(_, data) => !data.open && onClose()}>
      <DialogSurface>
        <DialogBody>
          <DialogTitle>New group</DialogTitle>
          <DialogContent>
            <Field label="Name" required>
              <Input value={name} onChange={(_, data) => onNameChange(data.value)} />
            </Field>
          </DialogContent>
          <DialogActions>
            <Button onClick={onClose}>Cancel</Button>
            <Button appearance="primary" onClick={onCreate}>Create</Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
