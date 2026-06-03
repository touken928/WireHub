import { Button, Input } from '@fluentui/react-components';
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, setToken } from '@/api';
import { LoginLayout } from '@/components/layout/LoginLayout';
import { getErrorMessage } from '@/lib/error';
import { loginInputFocusStyle, useLoginPageStyles } from '@/styles/loginPage';

type FocusField = 'username' | 'password' | null;

export default function LoginPage() {
  const styles = useLoginPageStyles();
  const navigate = useNavigate();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [focusedField, setFocusedField] = useState<FocusField>(null);

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    setLoading(true);
    setError('');
    try {
      const { token } = await api.login(username, password);
      setToken(token);
      navigate('/');
    } catch (err) {
      setError(getErrorMessage(err, 'Login failed'));
    } finally {
      setLoading(false);
    }
  };

  const inputFocusHandlers = (field: Exclude<FocusField, null>) => ({
    onFocus: () => setFocusedField(field),
    onBlur: () => setFocusedField((current) => (current === field ? null : current)),
  });

  return (
    <LoginLayout>
      <div>
        <h2 className={styles.formTitle}>Sign in</h2>
        <p className={styles.formSubtitle}>Enter your admin credentials to continue.</p>
      </div>

      <form onSubmit={handleSubmit} className={styles.form} autoComplete="off">
        <div className={styles.field}>
          <label className={styles.fieldLabel} htmlFor="login-username">
            Username
          </label>
          <Input
            id="login-username"
            name="username"
            placeholder="Admin username"
            value={username}
            autoComplete="off"
            className={styles.inputRoot}
            style={loginInputFocusStyle(focusedField === 'username')}
            input={{
              className: styles.inputField,
              ...inputFocusHandlers('username'),
            }}
            onChange={(_, data) => setUsername(data.value)}
          />
        </div>

        <div className={styles.field}>
          <label className={styles.fieldLabel} htmlFor="login-password">
            Password
          </label>
          <Input
            id="login-password"
            name="password"
            type="password"
            placeholder="Password"
            value={password}
            autoComplete="new-password"
            className={styles.inputRoot}
            style={loginInputFocusStyle(focusedField === 'password')}
            input={{
              className: styles.inputField,
              ...inputFocusHandlers('password'),
            }}
            onChange={(_, data) => setPassword(data.value)}
          />
        </div>

        {error ? (
          <div className={`${styles.errorBanner} login-animate-scale-in`} role="alert">
            {error}
          </div>
        ) : null}

        <Button
          appearance="primary"
          type="submit"
          disabled={loading}
          className={styles.submitButton}
        >
          {loading ? 'Signing in…' : 'Sign in'}
        </Button>
      </form>
    </LoginLayout>
  );
}
