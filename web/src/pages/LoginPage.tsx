import { Button } from '@fluentui/react-components';
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, setToken } from '@/api';
import { AuthField } from '@/components/auth/AuthField';
import { LoginLayout } from '@/components/layout/LoginLayout';
import { getErrorMessage } from '@/lib/error';
import { useLoginPageStyles } from '@/styles/loginPage';

export default function LoginPage() {
  const styles = useLoginPageStyles();
  const navigate = useNavigate();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const loginUsername = String(data.get('username') ?? username).trim();
    const loginPassword = String(data.get('password') ?? password);
    if (!loginUsername || !loginPassword) {
      setError('Username and password are required');
      return;
    }
    setLoading(true);
    setError('');
    try {
      const { token } = await api.login(loginUsername, loginPassword);
      setToken(token);
      navigate('/');
    } catch (err) {
      setError(getErrorMessage(err, 'Login failed'));
    } finally {
      setLoading(false);
    }
  };

  return (
    <LoginLayout>
      <div>
        <h2 className={styles.formTitle}>Sign in</h2>
        <p className={styles.formSubtitle}>Enter your admin credentials to continue.</p>
      </div>

      <form onSubmit={handleSubmit} className={styles.form}>
        <AuthField
          id="login-username"
          name="username"
          label="Username"
          placeholder="Admin username"
          autoComplete="username"
          required
          value={username}
          onChange={setUsername}
        />
        <AuthField
          id="login-password"
          name="password"
          label="Password"
          type="password"
          placeholder="Password"
          autoComplete="current-password"
          required
          value={password}
          onChange={setPassword}
        />

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
