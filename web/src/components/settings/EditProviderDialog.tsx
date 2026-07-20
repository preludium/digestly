import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
    Dialog,
    DialogContent,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

interface EditProviderDialogProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    initialName: string;
    initialModel: string;
    onSubmit: (name: string, model: string) => void;
}

export function EditProviderDialog({
    open,
    onOpenChange,
    initialName,
    initialModel,
    onSubmit,
}: EditProviderDialogProps) {
    const [name, setName] = useState(initialName);
    const [model, setModel] = useState(initialModel);

    useEffect(() => {
        if (open) {
            setName(initialName);
            setModel(initialModel);
        }
    }, [open, initialName, initialModel]);

    const submit = () => {
        const trimmedName = name.trim();
        const trimmedModel = model.trim();
        if (!trimmedName || !trimmedModel) return;
        onSubmit(trimmedName, trimmedModel);
        onOpenChange(false);
    };

    const canSubmit = name.trim().length > 0 && model.trim().length > 0;

    return (
        <Dialog open={open} onOpenChange={onOpenChange}>
            <DialogContent className="sm:max-w-sm">
                <DialogHeader>
                    <DialogTitle>Edit provider</DialogTitle>
                </DialogHeader>
                <form
                    className="space-y-4"
                    onSubmit={(e) => {
                        e.preventDefault();
                        submit();
                    }}
                >
                    <div className="space-y-1.5">
                        <Label htmlFor="edit-provider-name">
                            Provider name (account/project)
                        </Label>
                        <Input
                            id="edit-provider-name"
                            autoFocus
                            value={name}
                            placeholder="My LLM"
                            onChange={(e) => setName(e.target.value)}
                        />
                    </div>
                    <div className="space-y-1.5">
                        <Label htmlFor="edit-provider-model">Model</Label>
                        <Input
                            id="edit-provider-model"
                            value={model}
                            placeholder="e.g. gemini-3.5-flash"
                            onChange={(e) => setModel(e.target.value)}
                        />
                    </div>
                    <div className="flex justify-end gap-2">
                        <Button
                            type="button"
                            variant="ghost"
                            onClick={() => onOpenChange(false)}
                        >
                            Cancel
                        </Button>
                        <Button type="submit" disabled={!canSubmit}>
                            Save
                        </Button>
                    </div>
                </form>
            </DialogContent>
        </Dialog>
    );
}
