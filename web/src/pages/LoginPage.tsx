import {
  Input,
  Button,
  Title1,
  Text,
} from '@fluentui/react-components';
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, setToken } from '@/api';
import { AuthLayout } from '@/components/layout/AuthLayout';
import { getErrorMessage } from '@/lib/error';
import { useAuthLayoutStyles } from '@/styles/authLayout';

export default function LoginPage() {
  const styles = useAuthLayoutStyles();
  const navigate = useNavigate();
  const [username, setUsername] = useState('admin');
  const [password, setPassword] = useState('admin');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

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

  return (
    <AuthLayout>
      <Title1>WireHub</Title1>
      <Text>Centralized WireGuard Hub Manager</Text>
      <form onSubmit={handleSubmit} className={styles.form}>
        <Input
          placeholder="Username"
          value={username}
          onChange={(_, data) => setUsername(data.value)}
        />
        <Input
          type="password"
          placeholder="Password"
          value={password}
          onChange={(_, data) => setPassword(data.value)}
        />
        {error && <Text className={styles.error}>{error}</Text>}
        <Button appearance="primary" type="submit" disabled={loading}>
          {loading ? 'Signing in...' : 'Sign in'}
        </Button>
      </form>
    </AuthLayout>
  );
}
