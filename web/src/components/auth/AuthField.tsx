import { Input, Textarea, type InputProps } from '@fluentui/react-components';
import { useState } from 'react';
import { loginInputFocusStyle, useLoginPageStyles } from '@/styles/loginPage';

type AuthFieldBaseProps = {
  id: string;
  label: string;
  hint?: string;
  required?: boolean;
  name?: string;
  autoComplete?: string;
};

type AuthInputFieldProps = AuthFieldBaseProps & {
  multiline?: false;
  value: string;
  placeholder?: string;
  type?: InputProps['type'];
  onChange: (value: string) => void;
};

type AuthTextareaFieldProps = AuthFieldBaseProps & {
  multiline: true;
  value: string;
  rows?: number;
  onChange: (value: string) => void;
};

export type AuthFieldProps = AuthInputFieldProps | AuthTextareaFieldProps;

export function AuthField(props: AuthFieldProps) {
  const styles = useLoginPageStyles();
  const [focused, setFocused] = useState(false);
  const focusHandlers = {
    onFocus: () => setFocused(true),
    onBlur: () => setFocused(false),
  };

  return (
    <div className={styles.field}>
      <label className={styles.fieldLabel} htmlFor={props.id}>
        {props.label}
        {props.required ? ' *' : null}
      </label>
      {props.multiline ? (
        <Textarea
          id={props.id}
          value={props.value}
          rows={props.rows ?? 3}
          className={styles.textareaRoot}
          style={loginInputFocusStyle(focused)}
          textarea={{
            className: styles.textareaField,
            ...focusHandlers,
          }}
          onChange={(_, data) => props.onChange(data.value)}
        />
      ) : (
        <Input
          id={props.id}
          name={props.name}
          type={props.type}
          required={props.required}
          placeholder={props.placeholder}
          value={props.value}
          className={styles.inputRoot}
          style={loginInputFocusStyle(focused)}
          input={{
            className: styles.inputField,
            name: props.name,
            autoComplete: props.autoComplete,
            ...focusHandlers,
          }}
          onChange={(_, data) => props.onChange(data.value)}
        />
      )}
      {props.hint ? <span className={styles.fieldHint}>{props.hint}</span> : null}
    </div>
  );
}
