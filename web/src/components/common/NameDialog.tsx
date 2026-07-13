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

interface NameDialogProps {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    title: string;
    label: string;
    initialValue?: string;
    placeholder?: string;
    submitLabel?: string;
    allowEmpty?: boolean;
    onSubmit: (value: string) => void;
}

export function NameDialog({
    open,
    onOpenChange,
    title,
    label,
    initialValue = "",
    placeholder,
    submitLabel = "Save",
    allowEmpty = false,
    onSubmit,
}: NameDialogProps) {
    const [value, setValue] = useState(initialValue);
    useEffect(() => {
        if (open) setValue(initialValue);
    }, [open, initialValue]);

    const submit = () => {
        const trimmed = value.trim();
        if (!allowEmpty && !trimmed) return;
        onSubmit(trimmed);
        onOpenChange(false);
    };

    return (
        <Dialog open={open} onOpenChange={onOpenChange}>
            <DialogContent className="sm:max-w-sm">
                <DialogHeader>
                    <DialogTitle>{title}</DialogTitle>
                </DialogHeader>
                <form
                    className="space-y-4"
                    onSubmit={(e) => {
                        e.preventDefault();
                        submit();
                    }}
                >
                    <div className="space-y-1.5">
                        <Label htmlFor="name-dialog-input">{label}</Label>
                        <Input
                            id="name-dialog-input"
                            autoFocus
                            value={value}
                            placeholder={placeholder}
                            onChange={(e) => setValue(e.target.value)}
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
                        <Button
                            type="submit"
                            disabled={!allowEmpty && !value.trim()}
                        >
                            {submitLabel}
                        </Button>
                    </div>
                </form>
            </DialogContent>
        </Dialog>
    );
}
